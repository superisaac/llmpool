use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use super::client::{CreateMessageBatchParams, ListMessageBatchesResponse, MessageBatch};
use super::helpers::{
    AnthropicAppState, anthropic_sdk_get_raw, anthropic_sdk_get_request, anthropic_sdk_request,
    check_wallet_balance, select_anthropic_clients,
};
use crate::db;
use crate::middlewares::api_auth::ACCOUNT;

/// Generate a new UUIDv7-based batch_id with a "msgbatch-" prefix (Anthropic style).
fn new_batch_id() -> String {
    format!("msgbatch-{}", Uuid::now_v7().to_string().replace('-', ""))
}

/// Map an `anthropic_sdk::AnthropicError` to an Axum error response.
fn sdk_error_response(e: &anthropic_sdk::AnthropicError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": e.to_string()
            }
        })),
    )
        .into_response()
}

/// Spawn a task to mark an upstream offline when a network error is detected.
fn maybe_mark_offline(
    e: &anthropic_sdk::AnthropicError,
    pool: crate::db::DbPool,
    upstream_id: i64,
) {
    if matches!(e, anthropic_sdk::AnthropicError::Connection { .. }) {
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
}

/// Look up the BatchMeta for a given internal batch_id.
/// Returns an error Response if not found or DB error.
async fn resolve_batch_meta(
    state: &AnthropicAppState,
    batch_id: &str,
) -> Result<db::batches::BatchMeta, Response> {
    match db::batches::get_batch_meta_by_batch_id(&state.pool, batch_id).await {
        Ok(Some(meta)) => Ok(meta),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "not_found_error",
                    "message": format!("Message batch '{}' not found.", batch_id)
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(batch_id = %batch_id, error = %e, "DB error looking up anthropic batch meta");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Sync the batch processing_status from a MessageBatch to the batch_meta table.
async fn sync_batch_status(state: &AnthropicAppState, batch_id: &str, batch: &MessageBatch) {
    if let Err(e) =
        db::batches::update_batch_meta_status(&state.pool, batch_id, &batch.processing_status).await
    {
        warn!(batch_id = %batch_id, error = %e, "Failed to sync anthropic batch status to batch_meta");
    }
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

    match anthropic_sdk_request::<CreateMessageBatchParams, MessageBatch>(
        &upstream_client.client,
        "/v1/messages/batches",
        &payload,
    )
    .await
    {
        Ok(mut batch) => {
            // Generate our own batch_id and store the mapping
            let our_batch_id = new_batch_id();
            let original_batch_id = batch.id.clone();

            match db::batches::create_batch_meta_with_provider(
                &state.pool,
                &our_batch_id,
                &original_batch_id,
                upstream_client.upstream_id,
                "anthropic",
            )
            .await
            {
                Ok(_) => {
                    info!(
                        batch_id = %our_batch_id,
                        original_batch_id = %original_batch_id,
                        upstream_id = %upstream_client.upstream_id,
                        "Anthropic message batch created and meta stored"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to store anthropic batch meta in DB");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }

            // Replace the upstream batch_id with our own in the response
            batch.id = our_batch_id.clone();

            // Sync batch status to batch_meta
            sync_batch_status(&state, &our_batch_id, &batch).await;

            info!(
                batch_id = %our_batch_id,
                processing_status = %batch.processing_status,
                "Anthropic message batch created"
            );
            Json(batch).into_response()
        }
        Err(e) => {
            maybe_mark_offline(&e, state.pool.clone(), upstream_client.upstream_id);
            warn!(error = %e, "Anthropic message batch creation failed");
            sdk_error_response(&e)
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

    match anthropic_sdk_get_request::<ListMessageBatchesResponse>(
        &upstream_client.client,
        "/v1/messages/batches",
    )
    .await
    {
        Ok(list) => Json(list).into_response(),
        Err(e) => {
            maybe_mark_offline(&e, state.pool.clone(), upstream_client.upstream_id);
            warn!(error = %e, "Anthropic list message batches failed");
            sdk_error_response(&e)
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches/:message_batch_id — Retrieve a Message Batch
// ---------------------------------------------------------------------------

/// GET /v1/messages/batches/:message_batch_id — retrieve a specific message batch
pub async fn retrieve_message_batch(
    State(state): State<Arc<AnthropicAppState>>,
    Path(message_batch_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_batch_meta(&state, &message_batch_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

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
    let path = format!("/v1/messages/batches/{}", meta.original_batch_id);

    info!(
        batch_id = %message_batch_id,
        original_batch_id = %meta.original_batch_id,
        "Retrieving anthropic message batch"
    );

    match anthropic_sdk_get_request::<MessageBatch>(&upstream_client.client, &path).await {
        Ok(mut batch) => {
            // Replace upstream batch_id with our own in the response
            batch.id = message_batch_id.clone();

            // Sync batch status to batch_meta
            sync_batch_status(&state, &message_batch_id, &batch).await;

            Json(batch).into_response()
        }
        Err(e) => {
            maybe_mark_offline(&e, state.pool.clone(), upstream_client.upstream_id);
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic retrieve message batch failed");
            sdk_error_response(&e)
        }
    }
}

// ---------------------------------------------------------------------------
// POST /v1/messages/batches/:message_batch_id/cancel — Cancel a Message Batch
// ---------------------------------------------------------------------------

/// POST /v1/messages/batches/:message_batch_id/cancel — cancel a message batch
pub async fn cancel_message_batch(
    State(state): State<Arc<AnthropicAppState>>,
    Path(message_batch_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_batch_meta(&state, &message_batch_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

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
    let path = format!("/v1/messages/batches/{}/cancel", meta.original_batch_id);

    info!(
        batch_id = %message_batch_id,
        original_batch_id = %meta.original_batch_id,
        "Cancelling anthropic message batch"
    );

    // Cancel is a POST with an empty body
    match anthropic_sdk_request::<serde_json::Value, MessageBatch>(
        &upstream_client.client,
        &path,
        &serde_json::Value::Null,
    )
    .await
    {
        Ok(mut batch) => {
            // Replace upstream batch_id with our own in the response
            batch.id = message_batch_id.clone();

            // Sync batch status to batch_meta
            sync_batch_status(&state, &message_batch_id, &batch).await;

            info!(batch_id = %message_batch_id, "Anthropic message batch cancelled");
            Json(batch).into_response()
        }
        Err(e) => {
            maybe_mark_offline(&e, state.pool.clone(), upstream_client.upstream_id);
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic cancel message batch failed");
            sdk_error_response(&e)
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches/:message_batch_id/results — Retrieve Batch Results
// ---------------------------------------------------------------------------

/// GET /v1/messages/batches/:message_batch_id/results — stream batch results (JSONL)
pub async fn retrieve_message_batch_results(
    State(state): State<Arc<AnthropicAppState>>,
    Path(message_batch_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_batch_meta(&state, &message_batch_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

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
    let path = format!("/v1/messages/batches/{}/results", meta.original_batch_id);

    info!(
        batch_id = %message_batch_id,
        original_batch_id = %meta.original_batch_id,
        "Retrieving anthropic message batch results"
    );

    match anthropic_sdk_get_raw(&upstream_client.client, &path).await {
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
            maybe_mark_offline(&e, state.pool.clone(), upstream_client.upstream_id);
            warn!(batch_id = %message_batch_id, error = %e, "Anthropic retrieve batch results failed");
            sdk_error_response(&e)
        }
    }
}
