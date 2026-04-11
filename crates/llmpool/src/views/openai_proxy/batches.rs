use async_openai::types::batches::{Batch, BatchRequest};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use super::files::wrap_file;
use super::helpers::{AppState, build_client_from_upstream, check_wallet_balance};
use crate::db;
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::openai::session_tracer::SessionTracer;

/// Generate a new UUIDv7-based batch_id with a "batch-" prefix.
fn new_batch_id() -> String {
    format!("batch-{}", Uuid::now_v7().to_string().replace('-', ""))
}

/// Look up the upstream for a given upstream_id from the DB.
/// Returns an error Response if not found or DB error.
async fn get_upstream_by_id(
    state: &AppState,
    upstream_id: i64,
) -> Result<crate::models::LLMUpstream, Response> {
    match db::llm::get_upstream(&state.pool, upstream_id).await {
        Ok(upstream) => Ok(upstream),
        Err(sqlx::Error::RowNotFound) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": {
                    "message": format!("Upstream {} not found.", upstream_id),
                    "type": "server_error",
                    "code": "upstream_not_found"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(upstream_id = %upstream_id, error = %e, "DB error looking up upstream");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Look up the BatchMeta for a given internal batch_id.
/// Returns an error Response if not found or DB error.
async fn resolve_batch_meta(
    state: &AppState,
    batch_id: &str,
) -> Result<db::batches::BatchMeta, Response> {
    match db::batches::get_batch_meta_by_batch_id(&state.pool, batch_id).await {
        Ok(Some(meta)) => Ok(meta),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "message": format!("Batch '{}' not found.", batch_id),
                    "type": "invalid_request_error",
                    "code": "batch_not_found"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(batch_id = %batch_id, error = %e, "DB error looking up batch meta");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Sync the batch status from a Batch object to the batch_meta table.
async fn sync_batch_status(state: &AppState, batch_id: &str, batch: &Batch) {
    let status_str = serde_json::to_value(&batch.status)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if let Err(e) = db::batches::update_batch_meta_status(&state.pool, batch_id, &status_str).await
    {
        warn!(batch_id = %batch_id, error = %e, "Failed to sync batch status to batch_meta");
    }
}

/// Handle GET /v1/batches — list batches
pub async fn list_batches_handler(State(state): State<Arc<AppState>>) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Use the first available upstream for listing
    let upstream = match db::llm::list_upstreams(&state.pool).await {
        Ok(upstreams) if !upstreams.is_empty() => upstreams.into_iter().next().unwrap(),
        Ok(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": {
                        "message": "No upstream upstreams configured.",
                        "type": "server_error",
                        "code": "no_upstream"
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(error = %e, "Failed to query upstreams for batches proxy");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let client = build_client_from_upstream(&upstream);
    info!(upstream_name = %upstream.name, "Listing batches");

    match client.batches().list().await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to list batches");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle POST /v1/batches — create a new batch
pub async fn create_batch_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BatchRequest>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Resolve the upstream from the input_file_id's FileMeta
    let file_meta = match db::files::get_file_meta_by_file_id(&state.pool, &payload.input_file_id)
        .await
    {
        Ok(Some(meta)) => meta,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("File '{}' not found.", payload.input_file_id),
                        "type": "invalid_request_error",
                        "code": "file_not_found"
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(file_id = %payload.input_file_id, error = %e, "DB error looking up file meta for batch");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let upstream = match get_upstream_by_id(&state, file_meta.upstream_id).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    // Replace our internal file_id with the upstream's original_file_id before forwarding
    let mut upstream_payload = payload.clone();
    upstream_payload.input_file_id = file_meta.original_file_id.clone();

    let api_key_id = API_CREDENTIAL.with(|k| k.id);
    let mut tracer = SessionTracer::new(
        state.event_storage.clone(),
        account_id,
        upstream.id,
        api_key_id,
    );

    // Trace the incoming BatchRequest
    tracer
        .add(OpenAIEventData::BatchRequest(payload.clone()))
        .await;

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        input_file_id = %payload.input_file_id,
        original_file_id = %file_meta.original_file_id,
        "Creating batch"
    );

    match client.batches().create(upstream_payload).await {
        Ok(mut batch) => {
            // Generate our own batch_id and store the mapping
            let our_batch_id = new_batch_id();
            let original_batch_id = batch.id.clone();

            match db::batches::create_batch_meta_with_provider(
                &state.pool,
                &our_batch_id,
                &original_batch_id,
                upstream.id,
                "openai",
            )
            .await
            {
                Ok(_) => {
                    info!(
                        batch_id = %our_batch_id,
                        original_batch_id = %original_batch_id,
                        upstream_id = %upstream.id,
                        "Batch created and meta stored"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to store batch meta in DB");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }

            // Replace the upstream batch_id with our own in the response
            batch.id = our_batch_id.clone();

            // Sync batch status to batch_meta
            sync_batch_status(&state, &our_batch_id, &batch).await;

            // Wrap output_file_id if present
            batch.output_file_id =
                match wrap_file(&state, batch.output_file_id.clone(), upstream.id).await {
                    Ok(meta) => meta.map(|m| m.file_id),
                    Err(resp) => return resp,
                };

            // Trace the Batch response
            tracer.add(OpenAIEventData::Batch(batch.clone())).await;

            Json(batch).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to create batch");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle GET /v1/batches/:batch_id — retrieve a specific batch
pub async fn batch_by_id_handler(
    State(state): State<Arc<AppState>>,
    Path(batch_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_batch_meta(&state, &batch_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let upstream = match get_upstream_by_id(&state, meta.upstream_id).await {
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

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        batch_id = %batch_id,
        original_batch_id = %meta.original_batch_id,
        "Retrieving batch"
    );

    match client.batches().retrieve(&meta.original_batch_id).await {
        Ok(mut batch) => {
            // Replace upstream batch_id with our own in the response
            batch.id = batch_id.clone();

            // Sync batch status to batch_meta
            sync_batch_status(&state, &batch_id, &batch).await;

            // Wrap output_file_id if present
            batch.output_file_id =
                match wrap_file(&state, batch.output_file_id.clone(), meta.upstream_id).await {
                    Ok(m) => m.map(|m| m.file_id),
                    Err(resp) => return resp,
                };

            // Trace the Batch response
            tracer.add(OpenAIEventData::Batch(batch.clone())).await;

            Json(batch).into_response()
        }
        Err(e) => {
            warn!(batch_id = %batch_id, error = %e, "Failed to retrieve batch");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle POST /v1/batches/:batch_id/cancel — cancel a batch
pub async fn batch_cancel_handler(
    State(state): State<Arc<AppState>>,
    Path(batch_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_batch_meta(&state, &batch_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let upstream = match get_upstream_by_id(&state, meta.upstream_id).await {
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

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        batch_id = %batch_id,
        original_batch_id = %meta.original_batch_id,
        "Cancelling batch"
    );

    match client.batches().cancel(&meta.original_batch_id).await {
        Ok(mut batch) => {
            // Replace upstream batch_id with our own in the response
            batch.id = batch_id;

            // Wrap output_file_id if present
            batch.output_file_id =
                match wrap_file(&state, batch.output_file_id.clone(), meta.upstream_id).await {
                    Ok(m) => m.map(|m| m.file_id),
                    Err(resp) => return resp,
                };

            // Trace the cancelled Batch response
            tracer.add(OpenAIEventData::Batch(batch.clone())).await;

            Json(batch).into_response()
        }
        Err(e) => {
            warn!(batch_id = %batch_id, error = %e, "Failed to cancel batch");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
