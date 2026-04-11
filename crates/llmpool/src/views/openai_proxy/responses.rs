use async_openai::{
    Client,
    config::OpenAIConfig,
    types::responses::{CreateResponse, Response, ResponseStreamEvent},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::sse::Event,
    response::{IntoResponse, Response as AxumResponse},
};
use futures::stream::{Stream, StreamExt};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::db;
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::openai::session_tracer::SessionTracer;

/// Generate a new UUIDv7-based response_id with a "resp-" prefix.
fn new_response_id() -> String {
    format!("resp-{}", Uuid::now_v7().to_string().replace('-', ""))
}

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

    let clients = select_model_clients(
        &state.pool,
        &state.redis_pool,
        &model_name,
        "openai",
        crate::openai::features::FEATURE_RESPONSES,
        2,
    )
    .await;
    if clients.is_empty() {
        eprintln!("No client for model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    // If previous_response_id is set, translate our internal ID to the upstream's original ID
    let our_previous_response_id = payload.previous_response_id.clone();
    let original_previous_response_id = if let Some(ref prev_id) = our_previous_response_id {
        match db::responses::get_original_response_id(&state.pool, prev_id).await {
            Ok(Some(orig_id)) => Some(orig_id),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Previous response '{}' not found.", prev_id),
                            "type": "invalid_request_error",
                            "code": "response_not_found"
                        }
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                warn!(prev_id = %prev_id, error = %e, "DB error looking up previous response meta");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        None
    };

    for (i, upstream_client) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            upstream_client.model_db_id,
            api_key_id,
        );
        let mut upstream_payload = payload.clone();
        upstream_payload.model = Some(upstream_client.fullname.clone());
        // Replace our internal previous_response_id with the upstream's original ID
        upstream_payload.previous_response_id = original_previous_response_id.clone();
        let upstream_id = upstream_client.upstream_id;
        match create_response_with_client(&upstream_client.client, &mut tracer, upstream_payload)
            .await
        {
            Ok((response_json, original_response_id)) => {
                // Generate our own response_id and store the mapping in DB
                let our_response_id = new_response_id();
                match db::responses::create_response_meta(
                    &state.pool,
                    &our_response_id,
                    &original_response_id,
                    upstream_id,
                )
                .await
                {
                    Ok(_) => {
                        info!(
                            response_id = %our_response_id,
                            original_response_id = %original_response_id,
                            upstream_id = %upstream_id,
                            "Response meta stored"
                        );
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to store response meta in DB");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
                // Replace the upstream response id with our own before returning
                let mut value = response_json;
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("id".to_string(), serde_json::Value::String(our_response_id));
                    // Replace previous_response_id back to our internal ID
                    if let Some(ref our_prev_id) = our_previous_response_id {
                        obj.insert(
                            "previous_response_id".to_string(),
                            serde_json::Value::String(our_prev_id.clone()),
                        );
                    }
                }
                return Json(value).into_response();
            }
            Err(e) => {
                // On network errors, mark the upstream as offline
                if is_network_error(&e) {
                    let pool = state.pool.clone();
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

/// Handle DELETE /responses/:response_id — delete a model response by ID
pub async fn delete_response(
    State(state): State<Arc<AppState>>,
    Path(response_id): Path<String>,
) -> AxumResponse {
    // Look up the ResponseMeta to find the upstream and original_response_id
    let meta =
        match db::responses::get_response_meta_by_response_id(&state.pool, &response_id).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Response '{}' not found.", response_id),
                            "type": "invalid_request_error",
                            "code": "response_not_found"
                        }
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                warn!(response_id = %response_id, error = %e, "DB error looking up response meta");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

    // Fetch the upstream using the stored upstream_id
    let upstream = match db::llm::get_upstream(&state.pool, meta.upstream_id).await {
        Ok(u) => u,
        Err(sqlx::Error::RowNotFound) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("Upstream {} not found.", meta.upstream_id),
                        "type": "server_error",
                        "code": "upstream_not_found"
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(upstream_id = %meta.upstream_id, error = %e, "DB error looking up upstream");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let config = OpenAIConfig::new()
        .with_api_key(upstream.api_key.clone())
        .with_api_base(upstream.api_base.clone());
    let client = Client::with_config(config);

    info!(
        upstream_name = %upstream.name,
        response_id = %response_id,
        original_response_id = %meta.original_response_id,
        "Deleting response"
    );

    match client.responses().delete(&meta.original_response_id).await {
        Ok(deleted) => {
            // Mark the ResponseMeta as deleted in our DB
            if let Err(e) =
                db::responses::mark_response_meta_deleted(&state.pool, &response_id).await
            {
                warn!(response_id = %response_id, error = %e, "Failed to mark response meta as deleted");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            info!(response_id = %response_id, "Response deleted and marked as deleted in DB");
            Json(deleted).into_response()
        }
        Err(e) => {
            warn!(response_id = %response_id, error = %e, "Failed to delete response from upstream");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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

    // Look up the ResponseMeta to find the upstream and original_response_id
    let meta =
        match db::responses::get_response_meta_by_response_id(&state.pool, &response_id).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Response '{}' not found.", response_id),
                            "type": "invalid_request_error",
                            "code": "response_not_found"
                        }
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                warn!(response_id = %response_id, error = %e, "DB error looking up response meta");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

    // Fetch the upstream using the stored upstream_id
    let upstream = match db::llm::get_upstream(&state.pool, meta.upstream_id).await {
        Ok(u) => u,
        Err(sqlx::Error::RowNotFound) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("Upstream {} not found.", meta.upstream_id),
                        "type": "server_error",
                        "code": "upstream_not_found"
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(upstream_id = %meta.upstream_id, error = %e, "DB error looking up upstream");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
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
        original_response_id = %meta.original_response_id,
        "Retrieving response"
    );

    match client
        .responses()
        .retrieve(&meta.original_response_id)
        .await
    {
        Ok(mut response) => {
            log_response_usage(&response);

            // Trace the response
            tracer
                .add(OpenAIEventData::ResponsesResponse(response.clone()))
                .await;

            // Replace the upstream response id with our own before returning
            response.id = response_id;

            // Replace previous_response_id (upstream original) back to our internal ID
            if let Some(ref upstream_prev_id) = response.previous_response_id.clone() {
                match db::responses::get_response_id_from_original_response_id(
                    &state.pool,
                    upstream_prev_id,
                )
                .await
                {
                    Ok(Some(our_prev_id)) => {
                        response.previous_response_id = Some(our_prev_id);
                    }
                    Ok(None) => {
                        // No mapping found; leave as-is (may be an external ID)
                        warn!(
                            upstream_prev_id = %upstream_prev_id,
                            "No ResponseMeta found for upstream previous_response_id"
                        );
                    }
                    Err(e) => {
                        warn!(
                            upstream_prev_id = %upstream_prev_id,
                            error = %e,
                            "DB error looking up previous response meta by original_response_id"
                        );
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
            }

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
/// Returns Ok((response_json, original_response_id)) on success, Err on failure so the caller can retry.
async fn create_response_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateResponse,
) -> Result<(serde_json::Value, String), async_openai::error::OpenAIError> {
    // Log the incoming request
    tracer
        .add(OpenAIEventData::ResponsesRequest(payload.clone()))
        .await;

    let response = client.responses().create(payload).await?;
    log_response_usage(&response);

    // Log the response
    tracer
        .add(OpenAIEventData::ResponsesResponse(response.clone()))
        .await;

    let original_response_id = response.id.clone();
    let response_json = serde_json::to_value(&response).unwrap_or_default();
    Ok((response_json, original_response_id))
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

/// Transform async-openai response stream into Axum SSE event stream with session logging.
#[allow(dead_code)]
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
