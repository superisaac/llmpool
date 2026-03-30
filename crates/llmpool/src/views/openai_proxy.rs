use async_openai::{
    Client,
    config::OpenAIConfig,
    types::audio::CreateSpeechRequest,
    // types::videos::{CreateVideoRequest},
    types::chat::{CreateChatCompletionRequest, CreateChatCompletionStreamResponse},
    types::embeddings::CreateEmbeddingRequest,
    types::images::CreateImageRequest,
    types::models::{ListModelResponse, Model},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bigdecimal::BigDecimal;
use futures::stream::{Stream, StreamExt};
use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, warn};

use apalis_redis::RedisStorage;

use crate::db::{self, DbPool, RedisPool};
use crate::defer::{OpenAIEventData, OpenAIEventTask};
//use crate::models::OpenAIEventData;
use crate::models::{Account, ApiCredential, CapacityOption};
use crate::openai::session_tracer::SessionTracer;
use crate::redis_utils::caches::apikey::{self as redis_cache, ApiKeyInfo};
use crate::redis_utils::counters::get_output_token_usage_batch;

tokio::task_local! {
    pub static ACCOUNT: Account;
    pub static API_CREDENTIAL: ApiCredential;
}

// --- Server State ---

struct AppState {
    pool: DbPool,
    redis_pool: RedisPool,
    event_storage: RedisStorage<OpenAIEventTask>,
}

// --- Auth Middleware ---

/// Helper to build a JSON error response for authentication failures.
fn auth_error_response(status: StatusCode, message: &str, code: &str) -> Response {
    let error_type = if status == StatusCode::UNAUTHORIZED {
        "authentication_error"
    } else {
        "server_error"
    };
    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": code
            }
        })),
    )
        .into_response()
}

/// Middleware that authenticates requests using Bearer token from the Authorization header.
/// It looks up the ACCESS_KEY by apikey (checking Redis cache first, then DB on miss),
/// checks that it is active, then finds the account and checks that it is active.
/// Both ACCESS_KEY and ACCOUNT are stored in task-local variables for downstream handlers.
async fn auth_openai_api(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    // Extract the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Missing or invalid Authorization header. Expected: Bearer <apikey>",
                "invalid_api_key",
            );
        }
    };

    // Step 1: Try Redis cache first for apikey info
    let cached_info = match redis_cache::get_apikey_info(&state.redis_pool, token).await {
        Ok(info) => info,
        Err(e) => {
            // Cache error is non-fatal; fall through to DB lookup
            warn!(error = %e, "Redis cache error during apikey lookup, falling back to DB");
            None
        }
    };

    if let Some(info) = cached_info {
        // Validate cached info
        if !info.is_active {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Invalid API key.",
                "invalid_api_credential",
            );
        }
        let account_id = match info.account_id {
            Some(id) => id,
            None => {
                return auth_error_response(
                    StatusCode::UNAUTHORIZED,
                    "API key is not associated with an account.",
                    "invalid_api_credential",
                );
            }
        };
        if !info.account_is_active {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Account is inactive.",
                "invalid_api_credential",
            );
        }

        // Reconstruct ApiCredential and Account from cached info for task-locals
        let access_key =
            match db::api::find_active_api_credential_by_apikey(&state.pool, token).await {
                Ok(Some(key)) => key,
                Ok(None) => {
                    // Cache may be stale; invalidate and reject
                    let _ = redis_cache::delete_apikey(&state.redis_pool, token).await;
                    return auth_error_response(
                        StatusCode::UNAUTHORIZED,
                        "Invalid API key.",
                        "invalid_api_credential",
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Database error during API key lookup");
                    return auth_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal server error during authentication.",
                        "internal_error",
                    );
                }
            };
        let account = match db::account::get_account_by_id(&state.pool, account_id).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                let _ = redis_cache::delete_apikey(&state.redis_pool, token).await;
                return auth_error_response(
                    StatusCode::UNAUTHORIZED,
                    "Account not found for this API key.",
                    "invalid_api_credential",
                );
            }
            Err(e) => {
                warn!(error = %e, "Database error during account lookup");
                return auth_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error during authentication.",
                    "internal_error",
                );
            }
        };
        return API_CREDENTIAL
            .scope(access_key, ACCOUNT.scope(account, next.run(request)))
            .await;
    }

    // Step 1 (cache miss): Look up the API key from DB
    let access_key = match db::api::find_active_api_credential_by_apikey(&state.pool, token).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Invalid API key.",
                "invalid_api_credential",
            );
        }
        Err(e) => {
            warn!(error = %e, "Database error during API key lookup");
            return auth_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error during authentication.",
                "internal_error",
            );
        }
    };

    // Step 2: Find the account by ACCESS_KEY.account_id (if present)
    let account_id = match access_key.account_id {
        Some(uid) => uid,
        None => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "API key is not associated with an account.",
                "invalid_api_credential",
            );
        }
    };

    let account = match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Account not found for this API key.",
                "invalid_api_credential",
            );
        }
        Err(e) => {
            warn!(error = %e, "Database error during account lookup");
            return auth_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error during authentication.",
                "internal_error",
            );
        }
    };

    // Step 3: Check if the account is active
    if !account.is_active {
        return auth_error_response(
            StatusCode::UNAUTHORIZED,
            "Account is inactive.",
            "invalid_api_credential",
        );
    }

    // Step 4: Populate Redis cache for future requests
    let info = ApiKeyInfo {
        id: access_key.id,
        account_id: access_key.account_id,
        apikey: access_key.apikey.clone(),
        label: access_key.label.clone(),
        is_active: access_key.is_active,
        account_is_active: account.is_active,
    };
    if let Err(e) = redis_cache::set_apikey_info(&state.redis_pool, token, info).await {
        warn!(error = %e, "Failed to cache apikey info in Redis");
    }

    // Step 5: Set task-local variables and proceed
    API_CREDENTIAL
        .scope(access_key, ACCOUNT.scope(account, next.run(request)))
        .await
}

// --- Helpers ---

/// Check if the user has sufficient funds (available > 0).
/// Returns Ok(()) if funds are sufficient, or Err(Response) with an error response.
async fn check_fund_available(pool: &DbPool, account_id: i32) -> Result<(), Response> {
    let zero = BigDecimal::from(0);
    match db::fund::find_account_fund(pool, account_id).await {
        Ok(Some(fund)) if fund.available() > zero => Ok(()),
        Ok(Some(_fund)) => Err(insufficient_funds_response()),
        Ok(None) => {
            // No fund record means no balance
            Err(insufficient_funds_response())
        }
        Err(e) => {
            warn!(error = %e, "Database error during fund lookup");
            Err(auth_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error during fund check.",
                "internal_error",
            ))
        }
    }
}

/// Build a JSON error response for insufficient funds.
fn insufficient_funds_response() -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(serde_json::json!({
            "error": {
                "message": "Insufficient funds. Please add funds to your account to continue.",
                "type": "insufficient_funds",
                "code": "insufficient_funds"
            }
        })),
    )
        .into_response()
}

// --- Handlers ---
/// Handle /v1/models endpoint, return available model list from database
async fn list_merged_models(State(state): State<Arc<AppState>>) -> Response {
    let res = db::openai::list_models(&state.pool).await;

    match res {
        Ok(models) => {
            // Deduplicate by model_id, keeping the first occurrence
            let mut seen = HashSet::new();
            let unique_models: Vec<Model> = models
                .into_iter()
                .filter(|m| seen.insert(m.model_id.clone()))
                .map(|m| Model {
                    id: m.model_id,
                    object: "model".to_string(),
                    created: m.created_at.and_utc().timestamp() as u32,
                    owned_by: "system".to_string(),
                })
                .collect();

            let response = ListModelResponse {
                object: "list".to_string(),
                data: unique_models,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Models Error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Build a Client from an (LLMModel, LLMEndpoint) pair.
/// If the endpoint has proxies configured, a random one is selected and used.
fn build_client_from_model_endpoint(
    model: &crate::models::LLMModel,
    endpoint: &crate::models::LLMEndpoint,
) -> (Client<OpenAIConfig>, i32) {
    let config = OpenAIConfig::new()
        .with_api_key(endpoint.api_key.clone())
        .with_api_base(endpoint.api_base.clone());

    let client = if !endpoint.proxies.is_empty() {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if let Some(proxy_url) = endpoint.proxies.choose(&mut rng) {
            info!(
                endpoint_name = %endpoint.name,
                proxy = %proxy_url,
                "OpenAI proxy: using proxy for endpoint"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            let http_client = reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy");
            Client::with_config(config).with_http_client(http_client)
        } else {
            Client::with_config(config)
        }
    } else {
        Client::with_config(config)
    };

    (client, model.id)
}

/// Returns up to `count` (Client, model_db_id) pairs selected by lowest current-hour output
/// token usage from Redis. If a model has no Redis key, its usage defaults to 0.
async fn select_model_clients(
    db_pool: &DbPool,
    redis_pool: &RedisPool,
    model_name: &str,
    capacity: &CapacityOption,
    count: usize,
) -> Vec<(Client<OpenAIConfig>, i32)> {
    let models =
        match db::openai::find_models_by_name_and_capacity(db_pool, model_name, capacity).await {
            Ok(models) if !models.is_empty() => models,
            Ok(_) => {
                warn!(
                    model = model_name,
                    "No models found in DB for the requested capacity"
                );
                return vec![];
            }
            Err(e) => {
                warn!(
                    model = model_name,
                    error = %e,
                    "DB query failed when looking up models"
                );
                return vec![];
            }
        };

    // Fetch output token usage from Redis for all models in a single MGET, then sort ascending
    // and take the `count` with the lowest usage.
    let model_ids: Vec<i32> = models.iter().map(|(m, _)| m.id).collect();
    let usages = get_output_token_usage_batch(redis_pool, &model_ids).await;

    let mut models_with_usage: Vec<(i64, &(crate::models::LLMModel, crate::models::LLMEndpoint))> =
        usages.into_iter().zip(models.iter()).collect();

    models_with_usage.sort_by_key(|(usage, _)| *usage);
    models_with_usage.truncate(count);

    models_with_usage
        .into_iter()
        .map(|(usage, (model, endpoint))| {
            info!(
                model = model_name,
                endpoint_name = endpoint.name,
                api_base = endpoint.api_base,
                output_token_usage = usage,
                "Selected endpoint candidate by lowest output token usage"
            );
            build_client_from_model_endpoint(model, endpoint)
        })
        .collect()
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateChatCompletionRequest>,
) -> Response {
    let model_name = &payload.model;
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_available(&state.pool, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        has_chat_completion: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, (client, model_db_id)) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            *model_db_id,
            api_key_id,
        );
        match chat_completions_with_client(client, &mut tracer, payload.clone()).await {
            Ok(response) => return response,
            Err(e) => {
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Chat completion failed, retrying with another endpoint"
                    );
                } else {
                    eprintln!("Chat completion failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Execute a chat completion request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn chat_completions_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateChatCompletionRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    let is_stream = payload.stream.unwrap_or(false);

    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateChatCompletionRequest(
            payload.clone(),
        ))
        .await;

    if is_stream {
        let stream = client.chat().create_stream(payload).await?;
        let tracer = tracer.clone();
        let event_stream = transform_stream_with_logging(stream, tracer);
        Ok(Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        let response = client.chat().create(payload).await?;

        if let Some(ref usage) = response.usage {
            info!(
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                total_tokens = usage.total_tokens,
                "Chat completion usage"
            );
        }

        // Log the response
        tracer
            .add(OpenAIEventData::CreateChatCompletionResponse(
                response.clone(),
            ))
            .await;

        Ok(Json(response).into_response())
    }
}

/// Handle POST /v1/images/generations endpoint (image generation)
async fn generate_images(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateImageRequest>,
) -> axum::response::Response {
    let model_name = image_model_to_string(&payload.model);
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_available(&state.pool, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        has_image_generation: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for image model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, (client, model_db_id)) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            *model_db_id,
            api_key_id,
        );
        match generate_images_with_client(client, &mut tracer, payload.clone()).await {
            Ok(response) => return response,
            Err(e) => {
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Image generation failed, retrying with another endpoint"
                    );
                } else {
                    eprintln!("Image generation failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Execute an image generation request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn generate_images_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateImageRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateImageRequest(payload.clone()))
        .await;

    let response = client.images().generate(payload).await?;

    if let Some(ref usage) = response.usage {
        info!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            total_tokens = usage.total_tokens,
            "Image generation usage"
        );
    }

    // Log the response
    tracer
        .add(OpenAIEventData::ImagesResponse(response.clone()))
        .await;

    Ok(Json(response).into_response())
}

/// Convert ImageModel enum to string
fn image_model_to_string(model: &Option<async_openai::types::images::ImageModel>) -> String {
    match model {
        Some(m) => match m {
            async_openai::types::images::ImageModel::GptImage1 => "gpt-image-1".to_string(),
            async_openai::types::images::ImageModel::GptImage1dot5 => "gpt-image-1.5".to_string(),
            async_openai::types::images::ImageModel::GptImage1Mini => {
                "gpt-image-1-mini".to_string()
            }
            async_openai::types::images::ImageModel::DallE2 => "dall-e-2".to_string(),
            async_openai::types::images::ImageModel::DallE3 => "dall-e-3".to_string(),
            async_openai::types::images::ImageModel::Other(s) => s.clone(),
        },
        None => "dall-e-2".to_string(), // default model
    }
}

/// Handle POST /v1/audio/speech endpoint (text-to-speech)
async fn create_speech(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateSpeechRequest>,
) -> Response {
    let model_name = speech_model_to_string(&payload.model);
    let capacity = CapacityOption {
        has_speech: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 1).await;
    if let Some((client, _model_db_id)) = clients.first() {
        return create_speech_with_client(client, payload).await;
    } else {
        eprintln!("No client for speech model {model_name}");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

/// Execute a speech request using the specified client
async fn create_speech_with_client(
    client: &Client<OpenAIConfig>,
    payload: CreateSpeechRequest,
) -> Response {
    let res = client.audio().speech().create(payload).await;

    match res {
        Ok(resp) => Response::builder()
            .header("Content-Type", "audio/mpeg")
            .body(axum::body::Body::from(resp.bytes))
            .unwrap(),
        Err(e) => {
            eprintln!("Speech Generation Error: {:?}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Convert SpeechModel enum to string
fn speech_model_to_string(model: &async_openai::types::audio::SpeechModel) -> String {
    match model {
        async_openai::types::audio::SpeechModel::Tts1 => "tts-1".to_string(),
        async_openai::types::audio::SpeechModel::Tts1Hd => "tts-1-hd".to_string(),
        async_openai::types::audio::SpeechModel::Gpt4oMiniTts => "gpt-4o-mini-tts".to_string(),
        async_openai::types::audio::SpeechModel::Other(s) => s.clone(),
    }
}

/// Handle POST /v1/embeddings endpoint
async fn create_embeddings(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateEmbeddingRequest>,
) -> Response {
    let model_name = &payload.model;
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_available(&state.pool, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        has_embedding: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for embedding model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, (client, model_db_id)) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            *model_db_id,
            api_key_id,
        );
        match create_embeddings_with_client(client, &mut tracer, payload.clone()).await {
            Ok(response) => return response,
            Err(e) => {
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Embedding creation failed, retrying with another endpoint"
                    );
                } else {
                    eprintln!("Embedding creation failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Execute an embedding request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn create_embeddings_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateEmbeddingRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateEmbeddingRequest(payload.clone()))
        .await;

    let response = client.embeddings().create(payload).await?;

    info!(
        prompt_tokens = response.usage.prompt_tokens,
        total_tokens = response.usage.total_tokens,
        "Embedding usage"
    );

    // Log the response
    tracer
        .add(OpenAIEventData::CreateEmbeddingResponse(response.clone()))
        .await;

    Ok(Json(response).into_response())
}

// Transform async-openai response stream into Axum SSE event stream with session logging
fn transform_stream_with_logging(
    mut stream: impl Stream<
        Item = Result<CreateChatCompletionStreamResponse, async_openai::error::OpenAIError>,
    > + Unpin
    + Send
    + 'static,
    mut tracer: SessionTracer,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    if let Some(ref usage) = response.usage {
                        info!(
                            prompt_tokens = usage.prompt_tokens,
                            completion_tokens = usage.completion_tokens,
                            total_tokens = usage.total_tokens,
                            "Chat completion stream usage"
                        );
                    }
                    // Log stream response chunk
                    tracer
                        .add(OpenAIEventData::CreateChatCompletionStreamResponse(response.clone()))
                        .await;

                    if let Ok(json_data) = serde_json::to_string(&response) {
                        yield Ok(Event::default().data(json_data));
                    }
                }
                Err(e) => {
                    eprintln!("Stream item error: {:?}", e);
                    // On error, we can choose to terminate or send an error event
                    break;
                }
            }
        }

        tracer
            .add(OpenAIEventData::CreateChatCompletionStreamResponseDone)
            .await;

        // Send the OpenAI-conventional end marker
        yield Ok(Event::default().data("[DONE]"));
    }
}

pub fn get_router(
    pool: DbPool,
    redis_pool: RedisPool,
    event_storage: RedisStorage<OpenAIEventTask>,
) -> Router {
    let state = Arc::new(AppState {
        pool,
        redis_pool,
        event_storage,
    });
    Router::new()
        .route("/models", get(list_merged_models))
        .route("/chat/completions", post(chat_completions))
        .route("/embeddings", post(create_embeddings))
        .route("/images/generations", post(generate_images))
        // Audio-related routes
        .route("/audio/speech", post(create_speech))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_openai_api,
        ))
        .with_state(state)
}
