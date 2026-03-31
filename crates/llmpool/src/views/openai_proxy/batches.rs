use async_openai::types::batches::BatchRequest;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use super::helpers::{
    ACCOUNT, AppState, build_client_from_upstream, check_fund_balance, select_first_upstream,
};

/// Handle GET /v1/batches — list batches
pub async fn list_batches_handler(State(state): State<Arc<AppState>>) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
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
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        input_file_id = %payload.input_file_id,
        "Creating batch"
    );

    match client.batches().create(payload).await {
        Ok(batch) => Json(batch).into_response(),
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
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        batch_id = %batch_id,
        "Retrieving batch"
    );

    match client.batches().retrieve(&batch_id).await {
        Ok(batch) => Json(batch).into_response(),
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
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        batch_id = %batch_id,
        "Cancelling batch"
    );

    match client.batches().cancel(&batch_id).await {
        Ok(batch) => Json(batch).into_response(),
        Err(e) => {
            warn!(batch_id = %batch_id, error = %e, "Failed to cancel batch");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
