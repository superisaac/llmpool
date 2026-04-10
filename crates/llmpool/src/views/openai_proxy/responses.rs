use async_openai::{
    Client,
    config::OpenAIConfig,
    types::responses::{CreateResponse, Response, ResponseStreamEvent},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response as AxumResponse},
};
use futures::stream::{Stream, StreamExt};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, warn};

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::db;
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::models::CapacityOption;
use crate::openai::session_tracer::SessionTracer;

/// Handle POST /responses — create a new model response
pub async fn create_response(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateResponse>,
) -> AxumResponse {
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let model_name = match &payload.model {
        Some(m) => m.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "message": "model is required",
                        "type": "invalid_request_error",
                        "code": "missing_model"
                    }
                })),
            )
                .into_response();
        }
    };

    let capacity = CapacityOption {
        has_responses_api: Some(true),
        ..Default::default()
    };

    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, upstream_client) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            upstream_client.model_db_id,
            api_key_id,
        );
        match create_response_with_client(&upstream_client.client, &mut tracer, payload.clone())
            .await
        {
            Ok(response) => return response,
            Err(e) => {
                // On network errors, mark the upstream as offline
                if is_network_error(&e) {
                    let pool = state.pool.clone();
                    let upstream_id = upstream_client.upstream_id;
                    tokio::spawn(async move {
                        if let Err(db_err) =
                            db::llm::mark_upstream_offline(&pool, upstream_id).await
                        {
                            warn!(
                                upstream_id = upstream_id,
                                error = %db_err,
                                "Failed to mark upstream as offline"
                            );
                        } else {
                            warn!(
                                upstream_id = upstream_id,
                                "Marked upstream as offline due to network error"
                            );
                        }
                    });
                }
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Response creation failed, retrying with another upstream"
                    );
                } else {
                    eprintln!("Response creation failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Handle GET /responses/:response_id — retrieve a model response by ID
pub async fn retrieve_response(
    State(state): State<Arc<AppState>>,
    Path(response_id): Path<String>,
) -> AxumResponse {
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Use the first available upstream that supports the responses API
    let upstream = match select_responses_upstream(&state).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let api_key_id = API_CREDENTIAL.with(|k| k.id);
    let mut tracer = SessionTracer::new(
        state.event_storage.clone(),
        account_id,
        upstream.id,
        api_key_id,
    );

    let config = OpenAIConfig::new()
        .with_api_key(upstream.api_key.clone())
        .with_api_base(upstream.api_base.clone());
    let client = Client::with_config(config);

    info!(
        upstream_name = %upstream.name,
        response_id = %response_id,
        "Retrieving response"
    );

    match client.responses().retrieve(&response_id).await {
        Ok(response) => {
            log_response_usage(&response);

            // Trace the response
            tracer
                .add(OpenAIEventData::ResponsesResponse(response.clone()))
                .await;

            Json(response).into_response()
        }
        Err(e) => {
            warn!(response_id = %response_id, error = %e, "Failed to retrieve response");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Returns true if the OpenAI error is a network/reqwest error.
fn is_network_error(e: &async_openai::error::OpenAIError) -> bool {
    matches!(e, async_openai::error::OpenAIError::Reqwest(_))
}

/// Execute a response creation request using the specified client.
/// Returns Ok(AxumResponse) on success, Err on failure so the caller can retry.
async fn create_response_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateResponse,
) -> Result<AxumResponse, async_openai::error::OpenAIError> {
    let is_stream = payload.stream.unwrap_or(false);

    // Log the incoming request
    tracer
        .add(OpenAIEventData::ResponsesRequest(payload.clone()))
        .await;

    if is_stream {
        let stream = client.responses().create_stream(payload).await?;
        let tracer = tracer.clone();
        let event_stream = transform_stream_with_logging(stream, tracer);
        Ok(Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        let response = client.responses().create(payload).await?;
        log_response_usage(&response);

        // Log the response
        tracer
            .add(OpenAIEventData::ResponsesResponse(response.clone()))
            .await;

        Ok(Json(response).into_response())
    }
}

/// Log token usage from a Response if available.
fn log_response_usage(response: &Response) {
    if let Some(ref usage) = response.usage {
        info!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            total_tokens = usage.total_tokens,
            "Response usage"
        );
    }
}

/// Select the first upstream that has the responses API enabled.
async fn select_responses_upstream(
    state: &AppState,
) -> Result<crate::models::LLMUpstream, AxumResponse> {
    match db::llm::list_upstreams(&state.pool).await {
        Ok(upstreams) if !upstreams.is_empty() => Ok(upstreams.into_iter().next().unwrap()),
        Ok(_) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": {
                    "message": "No upstream configured.",
                    "type": "server_error",
                    "code": "no_upstream"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(error = %e, "Failed to query upstreams for responses proxy");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Transform async-openai response stream into Axum SSE event stream with session logging.
fn transform_stream_with_logging(
    mut stream: impl Stream<Item = Result<ResponseStreamEvent, async_openai::error::OpenAIError>>
    + Unpin
    + Send
    + 'static,
    mut tracer: SessionTracer,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    // Log completed response events that contain usage info
                    if let ResponseStreamEvent::ResponseCompleted(ref completed) = event {
                        if let Some(ref usage) = completed.response.usage {
                            info!(
                                input_tokens = usage.input_tokens,
                                output_tokens = usage.output_tokens,
                                total_tokens = usage.total_tokens,
                                "Response stream completed with usage"
                            );
                        }
                        // Trace the completed response
                        tracer
                            .add(OpenAIEventData::ResponsesResponse(completed.response.clone()))
                            .await;
                    }

                    if let Ok(json_data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(json_data));
                    }
                }
                Err(e) => {
                    eprintln!("Response stream item error: {:?}", e);
                    break;
                }
            }
        }

        tracer
            .add(OpenAIEventData::ResponsesStreamDone)
            .await;

        // Send the OpenAI-conventional end marker
        yield Ok(Event::default().data("[DONE]"));
    }
}
