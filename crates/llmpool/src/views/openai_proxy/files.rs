use async_openai::types::InputSource;
use async_openai::types::files::{CreateFileRequest, FileInput, FilePurpose};
use axum::{
    Json,
    body::Bytes,
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db;
use super::helpers::{
    ACCOUNT, AppState, build_client_from_upstream, check_fund_balance, select_first_upstream,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_purpose(s: &str) -> FilePurpose {
    match s {
        "assistants" => FilePurpose::Assistants,
        "batch" => FilePurpose::Batch,
        "vision" => FilePurpose::Vision,
        "user_data" => FilePurpose::UserData,
        "evals" => FilePurpose::Evals,
        _ => FilePurpose::FineTune, // default / "fine-tune"
    }
}

/// Generate a new UUIDv7-based file_id with a "file-" prefix.
fn new_file_id() -> String {
    format!("file-{}", Uuid::now_v7().to_string().replace('-', ""))
}

/// Look up the FileMeta for a given internal file_id.
/// Returns an error Response if not found or DB error.
async fn resolve_file_meta(
    state: &AppState,
    file_id: &str,
) -> Result<db::files::FileMeta, Response> {
    match db::files::get_file_meta_by_file_id(&state.pool, file_id).await {
        Ok(Some(meta)) => Ok(meta),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "message": format!("File '{}' not found.", file_id),
                    "type": "invalid_request_error",
                    "code": "file_not_found"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "DB error looking up file meta");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

/// Get the upstream for a given upstream_id from the DB.
/// Returns an error Response if not found or DB error.
async fn get_upstream_by_id(
    state: &AppState,
    upstream_id: i32,
) -> Result<crate::models::LLMUpstream, Response> {
    match db::openai::get_upstream(&state.pool, upstream_id).await {
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

// ── handlers ─────────────────────────────────────────────────────────────────

/// Handle GET /v1/files — list files
pub async fn list_files_handler(State(state): State<Arc<AppState>>) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(upstream_name = %upstream.name, "Listing files");

    match client.files().list().await {
        Ok(response) => {
            info!(count = response.data.len(), "Listed files");
            Json(response).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to list files");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle POST /v1/files — upload a new file (multipart/form-data)
pub async fn create_file_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let upstream = match select_first_upstream(&state).await {
        Ok(ep) => ep,
        Err(resp) => return resp,
    };

    // Parse multipart fields: "file" and "purpose"
    let mut file_bytes: Option<Bytes> = None;
    let mut file_name: Option<String> = None;
    let mut purpose_str: Option<String> = None;

    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let field_name = field.name().unwrap_or("").to_string();
                match field_name.as_str() {
                    "file" => {
                        file_name = field
                            .file_name()
                            .map(|s| s.to_string())
                            .or_else(|| Some("upload.bin".to_string()));
                        match field.bytes().await {
                            Ok(b) => file_bytes = Some(b),
                            Err(e) => {
                                warn!(error = %e, "Failed to read file field bytes");
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(serde_json::json!({
                                        "error": {
                                            "message": format!("Failed to read file bytes: {}", e),
                                            "type": "invalid_request_error",
                                            "code": "bad_request"
                                        }
                                    })),
                                )
                                    .into_response();
                            }
                        }
                    }
                    "purpose" => match field.text().await {
                        Ok(t) => purpose_str = Some(t),
                        Err(e) => {
                            warn!(error = %e, "Failed to read purpose field");
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({
                                    "error": {
                                        "message": format!("Failed to read purpose field: {}", e),
                                        "type": "invalid_request_error",
                                        "code": "bad_request"
                                    }
                                })),
                            )
                                .into_response();
                        }
                    },
                    _ => {
                        // ignore unknown fields
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                warn!(error = %e, "Multipart parsing error");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("Multipart parsing error: {}", e),
                            "type": "invalid_request_error",
                            "code": "bad_request"
                        }
                    })),
                )
                    .into_response();
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "message": "Missing required field: file",
                        "type": "invalid_request_error",
                        "code": "bad_request"
                    }
                })),
            )
                .into_response();
        }
    };

    let filename = file_name.unwrap_or_else(|| "upload.bin".to_string());
    let purpose_raw = purpose_str.unwrap_or_else(|| "fine-tune".to_string());
    let purpose = parse_purpose(&purpose_raw);

    let request = CreateFileRequest {
        file: FileInput {
            source: InputSource::Bytes {
                filename: filename.clone(),
                bytes,
            },
        },
        purpose,
        expires_after: None,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        filename = %filename,
        "Uploading file"
    );

    match client.files().create(request).await {
        Ok(mut file) => {
            // Generate our own file_id and store the mapping
            let our_file_id = new_file_id();
            let original_file_id = file.id.clone();

            match db::files::create_file_meta(
                &state.pool,
                &our_file_id,
                &original_file_id,
                &purpose_raw,
                upstream.id,
            )
            .await
            {
                Ok(_) => {
                    info!(
                        file_id = %our_file_id,
                        original_file_id = %original_file_id,
                        upstream_id = %upstream.id,
                        "File uploaded and meta stored"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to store file meta in DB");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }

            // Replace the upstream file_id with our own in the response
            file.id = our_file_id;
            Json(file).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to upload file");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle GET /v1/files/:file_id — retrieve file metadata
pub async fn retrieve_file_handler(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_file_meta(&state, &file_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let upstream = match get_upstream_by_id(&state, meta.upstream_id).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        file_id = %file_id,
        original_file_id = %meta.original_file_id,
        "Retrieving file metadata"
    );

    match client.files().retrieve(&meta.original_file_id).await {
        Ok(mut file) => {
            // Replace upstream file_id with our own in the response
            file.id = file_id;
            Json(file).into_response()
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "Failed to retrieve file");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle DELETE /v1/files/:file_id — delete a file
pub async fn delete_file_handler(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_file_meta(&state, &file_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let upstream = match get_upstream_by_id(&state, meta.upstream_id).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        file_id = %file_id,
        original_file_id = %meta.original_file_id,
        "Deleting file"
    );

    match client.files().delete(&meta.original_file_id).await {
        Ok(mut response) => {
            // Mark as deleted in our DB
            if let Err(e) = db::files::mark_file_meta_deleted(&state.pool, &file_id).await {
                warn!(file_id = %file_id, error = %e, "Failed to mark file meta as deleted");
            }
            // Replace upstream file_id with our own in the response
            response.id = file_id;
            Json(response).into_response()
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "Failed to delete file");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Handle GET /v1/files/:file_id/content — retrieve file content (raw bytes)
pub async fn file_content_handler(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let meta = match resolve_file_meta(&state, &file_id).await {
        Ok(m) => m,
        Err(resp) => return resp,
    };

    let upstream = match get_upstream_by_id(&state, meta.upstream_id).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let client = build_client_from_upstream(&upstream);
    info!(
        upstream_name = %upstream.name,
        file_id = %file_id,
        original_file_id = %meta.original_file_id,
        "Retrieving file content"
    );

    match client.files().content(&meta.original_file_id).await {
        Ok(bytes) => {
            info!(
                file_id = %file_id,
                bytes = bytes.len(),
                "File content retrieved"
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/octet-stream")
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "Failed to retrieve file content");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
