use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use super::client::{AnthropicApiError, CreateMessageBatchParams};
use super::helpers::{AnthropicAppState, check_wallet_balance, select_anthropic_clients};
use crate::db;
use crate::middlewares::api_auth::ACCOUNT;

fn is_network_error(e: &AnthropicApiError) -> bool {
    matches!(e, AnthropicApiError::Network(_))
}

// ---------------------------------------------------------------------------
// POST /v1/messages/batches — Create a Message Batch
// ---------------------------------------------------------------------------

/// POST /v1/messages/batches — proxy batch creation to the configured Anthropic upstream
pub async fn create_message_batch(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CreateMessageBatchParams>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Use the first request's model to select an upstream
    let model_name = payload
        .requests
        .first()
        .map(|r| r.params.model.clone())
        .unwrap_or_default();

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, &model_name, 1).await;
    if clients.is_empty() {
        warn!(model = %model_name, "No anthropic upstream client found for model (batch)");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available for the requested model."
                }
            })),
        )
            .into_response();
    }

    let upstream_client = &clients[0];
    let api_client = &upstream_client.client;

    match api_client.create_message_batch(&payload).await {
        Ok(batch) => {
            info!(
                batch_id = %batch.id,
                processing_status = %batch.processing_status,
                "Anthropic message batch created"
            );
            Json(batch).into_response()
        }
        Err(e) => {
            if is_network_error(&e) {
                let pool = state.pool.clone();
                let upstream_id = upstream_client.upstream_id;
                tokio::spawn(async move {
                    if let Err(db_err) = db::llm::mark_upstream_offline(&pool, upstream_id).await {
                        warn!(
                            upstream_id = upstream_id,
                            error = %db_err,
                            "Failed to mark anthropic upstream as offline"
                        );
                    }
                });
            }
            warn!(error = %e, "Anthropic message batch creation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while proxying batch to upstream."
                    }
                })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches — List Message Batches
// ---------------------------------------------------------------------------

/// GET /v1/messages/batches — list message batches from the configured Anthropic upstream
pub async fn list_message_batches(State(state): State<Arc<AnthropicAppState>>) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Use any available upstream (no model filter needed for listing)
    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, "", 1).await;
    if clients.is_empty() {
        warn!("No anthropic upstream client found for listing batches");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available."
                }
            })),
        )
            .into_response();
    }

    let upstream_client = &clients[0];
    let api_client = &upstream_client.client;

    let params = super::client::ListMessageBatchesParams::default();
    match api_client.list_message_batches(&params).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => {
            warn!(error = %e, "Anthropic list message batches failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while listing batches."
                    }
                })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches/:message_batch_id — Retrieve a Message Batch
// ---------------------------------------------------------------------------

/// GET /v1/messages/batches/:message_batch_id — retrieve a specific message batch
pub async fn retrieve_message_batch(
    State(state): State<Arc<AnthropicAppState>>,
    axum::extract::Path(message_batch_id): axum::extract::Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, "", 1).await;
    if clients.is_empty() {
        warn!("No anthropic upstream client found for retrieving batch");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available."
                }
            })),
        )
            .into_response();
    }

    let upstream_client = &clients[0];
    let api_client = &upstream_client.client;

    match api_client.retrieve_message_batch(&message_batch_id).await {
        Ok(batch) => Json(batch).into_response(),
        Err(e) => {
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic retrieve message batch failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while retrieving batch."
                    }
                })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// POST /v1/messages/batches/:message_batch_id/cancel — Cancel a Message Batch
// ---------------------------------------------------------------------------

/// POST /v1/messages/batches/:message_batch_id/cancel — cancel a message batch
pub async fn cancel_message_batch(
    State(state): State<Arc<AnthropicAppState>>,
    axum::extract::Path(message_batch_id): axum::extract::Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, "", 1).await;
    if clients.is_empty() {
        warn!("No anthropic upstream client found for cancelling batch");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available."
                }
            })),
        )
            .into_response();
    }

    let upstream_client = &clients[0];
    let api_client = &upstream_client.client;

    match api_client.cancel_message_batch(&message_batch_id).await {
        Ok(batch) => {
            info!(batch_id = %message_batch_id, "Anthropic message batch cancelled");
            Json(batch).into_response()
        }
        Err(e) => {
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic cancel message batch failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while cancelling batch."
                    }
                })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches/:message_batch_id/results — Retrieve Batch Results
// ---------------------------------------------------------------------------

/// GET /v1/messages/batches/:message_batch_id/results — stream batch results (JSONL)
pub async fn retrieve_message_batch_results(
    State(state): State<Arc<AnthropicAppState>>,
    axum::extract::Path(message_batch_id): axum::extract::Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, "", 1).await;
    if clients.is_empty() {
        warn!("No anthropic upstream client found for batch results");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available."
                }
            })),
        )
            .into_response();
    }

    let upstream_client = &clients[0];
    let api_client = &upstream_client.client;

    match api_client
        .retrieve_message_batch_results_raw(&message_batch_id)
        .await
    {
        Ok(upstream_resp) => {
            // Stream the JSONL body directly back to the client
            let status = upstream_resp.status();
            let headers = upstream_resp.headers().clone();
            let body = axum::body::Body::from_stream(upstream_resp.bytes_stream());
            let mut response = axum::response::Response::new(body);
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            response
        }
        Err(e) => {
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic retrieve batch results failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while retrieving batch results."
                    }
                })),
            )
                .into_response()
        }
    }
}
