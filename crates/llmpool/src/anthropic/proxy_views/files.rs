//! Anthropic Files API proxy handlers
//!
//! Proxies `/v1/files` endpoints to the configured Anthropic upstream.
//! Endpoints:
//!   POST   /v1/files                  — Upload a file
//!   GET    /v1/files                  — List files
//!   GET    /v1/files/:file_id         — Retrieve file metadata
//!   DELETE /v1/files/:file_id         — Delete a file
//!   GET    /v1/files/:file_id/content — Download file content

use anthropic_sdk::{Anthropic, AnthropicError};
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

use super::helpers::{AnthropicAppState, check_wallet_balance, select_anthropic_clients};
use crate::db;
use crate::middlewares::api_auth::ACCOUNT;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map an `AnthropicError` to an Axum error response.
fn sdk_error_response(e: &AnthropicError) -> Response {
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
fn maybe_mark_offline(e: &AnthropicError, pool: crate::db::DbPool, upstream_id: i64) {
    if matches!(e, AnthropicError::Connection { .. }) {
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

/// Generate a new UUIDv7-based file_id with an "anthropic-file-" prefix.
fn new_file_id() -> String {
    format!(
        "anthropic-file-{}",
        Uuid::now_v7().to_string().replace('-', "")
    )
}

/// Build a reqwest HTTP client from an Anthropic SDK client's config,
/// applying proxy settings if configured.
fn build_http_client(client: &Anthropic) -> Result<reqwest::Client, AnthropicError> {
    let config = client.config();
    let mut builder = reqwest::Client::builder();
    if let Some(ref proxy_url) = config.proxy_url {
        if let Ok(proxy) = reqwest::Proxy::all(proxy_url.as_str()) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build().map_err(|e| AnthropicError::Connection {
        message: e.to_string(),
    })
}

/// Add the standard Anthropic auth headers to a request builder.
fn add_auth_headers(req: reqwest::RequestBuilder, client: &Anthropic) -> reqwest::RequestBuilder {
    use anthropic_sdk::AuthMethod;

    let config = client.config();
    match config.auth_method {
        AuthMethod::Bearer => req.header("authorization", format!("Bearer {}", config.api_key)),
        AuthMethod::Token => req.header("token", &config.api_key),
        _ => req.header("x-api-key", &config.api_key),
    }
}

/// Select the first available upstream client (no model filter needed for files).
async fn get_upstream_client(
    state: &AnthropicAppState,
) -> Result<super::helpers::AnthropicClientContext, Response> {
    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, "", 1).await;
    if clients.is_empty() {
        warn!("No anthropic upstream client found for files API");
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "overloaded_error",
                    "message": "No upstream available."
                }
            })),
        )
            .into_response());
    }
    Ok(clients.into_iter().next().unwrap())
}

// ---------------------------------------------------------------------------
// POST /v1/files — Upload a file (multipart/form-data)
// ---------------------------------------------------------------------------

/// POST /v1/files — upload a file to the Anthropic upstream
pub async fn upload_file(
    State(state): State<Arc<AnthropicAppState>>,
    mut multipart: Multipart,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let upstream_client = match get_upstream_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Parse multipart fields: "file" and "purpose"
    let mut file_bytes: Option<Bytes> = None;
    let mut file_name: Option<String> = None;
    let mut content_type_str: Option<String> = None;
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
                        content_type_str = field.content_type().map(|s| s.to_string());
                        match field.bytes().await {
                            Ok(b) => file_bytes = Some(b),
                            Err(e) => {
                                warn!(error = %e, "Failed to read file field bytes");
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(serde_json::json!({
                                        "type": "error",
                                        "error": {
                                            "type": "invalid_request_error",
                                            "message": format!("Failed to read file bytes: {}", e)
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
                                    "type": "error",
                                    "error": {
                                        "type": "invalid_request_error",
                                        "message": format!("Failed to read purpose field: {}", e)
                                    }
                                })),
                            )
                                .into_response();
                        }
                    },
                    _ => {} // ignore unknown fields
                }
            }
            Ok(None) => break,
            Err(e) => {
                warn!(error = %e, "Multipart parsing error");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": format!("Multipart parsing error: {}", e)
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
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "message": "Missing required field: file"
                    }
                })),
            )
                .into_response();
        }
    };

    let filename = file_name.unwrap_or_else(|| "upload.bin".to_string());
    let purpose = purpose_str.unwrap_or_else(|| "batch_input".to_string());
    let mime = content_type_str.unwrap_or_else(|| "application/octet-stream".to_string());

    // Build the multipart form and forward to upstream
    let http_client = match build_http_client(&upstream_client.client) {
        Ok(c) => c,
        Err(e) => return sdk_error_response(&e),
    };

    let config = upstream_client.client.config();
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{}/v1/files", base_url);

    let file_part = match reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(filename.clone())
        .mime_str(&mime)
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "Failed to build multipart part");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("purpose", purpose.clone());

    let req = http_client
        .post(&url)
        .header("anthropic-version", "2023-06-01")
        .multipart(form);
    let req = add_auth_headers(req, &upstream_client.client);

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let err = AnthropicError::Connection {
                message: e.to_string(),
            };
            maybe_mark_offline(&err, state.pool.clone(), upstream_client.upstream_id);
            warn!(error = %e, "Anthropic file upload request failed");
            return sdk_error_response(&err);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let body_text = response.text().await.unwrap_or_default();
        warn!(status = status_code, body = %body_text, "Anthropic file upload returned error");
        return (
            StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            axum::response::Response::builder()
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body_text))
                .unwrap(),
        )
            .into_response();
    }

    let mut file_obj: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Failed to parse Anthropic file upload response");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Store the file mapping in our DB and replace the upstream file_id
    let original_file_id = file_obj
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !original_file_id.is_empty() {
        let our_file_id = new_file_id();
        match db::files::create_file_meta(
            &state.pool,
            &our_file_id,
            &original_file_id,
            &purpose,
            upstream_client.upstream_id,
        )
        .await
        {
            Ok(_) => {
                info!(
                    file_id = %our_file_id,
                    original_file_id = %original_file_id,
                    upstream_id = %upstream_client.upstream_id,
                    "Anthropic file uploaded and meta stored"
                );
                file_obj["id"] = serde_json::Value::String(our_file_id);
            }
            Err(e) => {
                warn!(error = %e, "Failed to store Anthropic file meta in DB");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    Json(file_obj).into_response()
}

// ---------------------------------------------------------------------------
// GET /v1/files — List files
// ---------------------------------------------------------------------------

/// GET /v1/files — list files from the Anthropic upstream
pub async fn list_files(State(state): State<Arc<AnthropicAppState>>) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let upstream_client = match get_upstream_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let http_client = match build_http_client(&upstream_client.client) {
        Ok(c) => c,
        Err(e) => return sdk_error_response(&e),
    };

    let config = upstream_client.client.config();
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{}/v1/files", base_url);

    let req = http_client
        .get(&url)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    let req = add_auth_headers(req, &upstream_client.client);

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let err = AnthropicError::Connection {
                message: e.to_string(),
            };
            maybe_mark_offline(&err, state.pool.clone(), upstream_client.upstream_id);
            warn!(error = %e, "Anthropic list files request failed");
            return sdk_error_response(&err);
        }
    };

    let status = response.status();
    let body_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "Failed to read Anthropic list files response body");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    axum::response::Response::builder()
        .status(status.as_u16())
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body_bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ---------------------------------------------------------------------------
// GET /v1/files/:file_id — Retrieve file metadata
// ---------------------------------------------------------------------------

/// GET /v1/files/:file_id — retrieve file metadata from the Anthropic upstream
pub async fn retrieve_file(
    State(state): State<Arc<AnthropicAppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Resolve our internal file_id → original upstream file_id
    let meta = match db::files::get_file_meta_by_file_id(&state.pool, &file_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "not_found_error",
                        "message": format!("File '{}' not found.", file_id)
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "DB error looking up Anthropic file meta");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let upstream_client = match get_upstream_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let http_client = match build_http_client(&upstream_client.client) {
        Ok(c) => c,
        Err(e) => return sdk_error_response(&e),
    };

    let config = upstream_client.client.config();
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{}/v1/files/{}", base_url, meta.original_file_id);

    let req = http_client
        .get(&url)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    let req = add_auth_headers(req, &upstream_client.client);

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let err = AnthropicError::Connection {
                message: e.to_string(),
            };
            maybe_mark_offline(&err, state.pool.clone(), upstream_client.upstream_id);
            warn!(file_id = %file_id, error = %e, "Anthropic retrieve file request failed");
            return sdk_error_response(&err);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let body_text = response.text().await.unwrap_or_default();
        return (
            StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            axum::response::Response::builder()
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body_text))
                .unwrap(),
        )
            .into_response();
    }

    let mut file_obj: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "Failed to parse Anthropic retrieve file response");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Replace upstream file_id with our internal one
    file_obj["id"] = serde_json::Value::String(file_id);
    Json(file_obj).into_response()
}

// ---------------------------------------------------------------------------
// DELETE /v1/files/:file_id — Delete a file
// ---------------------------------------------------------------------------

/// DELETE /v1/files/:file_id — delete a file from the Anthropic upstream
pub async fn delete_file(
    State(state): State<Arc<AnthropicAppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Resolve our internal file_id → original upstream file_id
    let meta = match db::files::get_file_meta_by_file_id(&state.pool, &file_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "not_found_error",
                        "message": format!("File '{}' not found.", file_id)
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "DB error looking up Anthropic file meta");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let upstream_client = match get_upstream_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let http_client = match build_http_client(&upstream_client.client) {
        Ok(c) => c,
        Err(e) => return sdk_error_response(&e),
    };

    let config = upstream_client.client.config();
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{}/v1/files/{}", base_url, meta.original_file_id);

    let req = http_client
        .delete(&url)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    let req = add_auth_headers(req, &upstream_client.client);

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let err = AnthropicError::Connection {
                message: e.to_string(),
            };
            maybe_mark_offline(&err, state.pool.clone(), upstream_client.upstream_id);
            warn!(file_id = %file_id, error = %e, "Anthropic delete file request failed");
            return sdk_error_response(&err);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let body_text = response.text().await.unwrap_or_default();
        return (
            StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            axum::response::Response::builder()
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body_text))
                .unwrap(),
        )
            .into_response();
    }

    // Mark as deleted in our DB
    if let Err(e) = db::files::mark_file_meta_deleted(&state.pool, &file_id).await {
        warn!(file_id = %file_id, error = %e, "Failed to mark Anthropic file meta as deleted");
    } else {
        info!(file_id = %file_id, "Anthropic file deleted and meta marked as deleted");
    }

    let mut file_obj: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "Failed to parse Anthropic delete file response");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Replace upstream file_id with our internal one
    file_obj["id"] = serde_json::Value::String(file_id);
    Json(file_obj).into_response()
}

// ---------------------------------------------------------------------------
// GET /v1/files/:file_id/content — Download file content
// ---------------------------------------------------------------------------

/// GET /v1/files/:file_id/content — stream file content from the Anthropic upstream
pub async fn file_content(
    State(state): State<Arc<AnthropicAppState>>,
    Path(file_id): Path<String>,
) -> Response {
    let account_id = ACCOUNT.with(|u| u.id);
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    // Resolve our internal file_id → original upstream file_id
    let meta = match db::files::get_file_meta_by_file_id(&state.pool, &file_id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "not_found_error",
                        "message": format!("File '{}' not found.", file_id)
                    }
                })),
            )
                .into_response();
        }
        Err(e) => {
            warn!(file_id = %file_id, error = %e, "DB error looking up Anthropic file meta");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let upstream_client = match get_upstream_client(&state).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let http_client = match build_http_client(&upstream_client.client) {
        Ok(c) => c,
        Err(e) => return sdk_error_response(&e),
    };

    let config = upstream_client.client.config();
    let base_url = config.base_url.trim_end_matches('/');
    let url = format!("{}/v1/files/{}/content", base_url, meta.original_file_id);

    let req = http_client
        .get(&url)
        .header("anthropic-version", "2023-06-01");
    let req = add_auth_headers(req, &upstream_client.client);

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let err = AnthropicError::Connection {
                message: e.to_string(),
            };
            maybe_mark_offline(&err, state.pool.clone(), upstream_client.upstream_id);
            warn!(file_id = %file_id, error = %e, "Anthropic file content request failed");
            return sdk_error_response(&err);
        }
    };

    let status = response.status();
    let headers = response.headers().clone();

    if !status.is_success() {
        let status_code = status.as_u16();
        let body_text = response.text().await.unwrap_or_default();
        warn!(file_id = %file_id, status = status_code, "Anthropic file content returned error");
        return (
            StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            axum::response::Response::builder()
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body_text))
                .unwrap(),
        )
            .into_response();
    }

    info!(file_id = %file_id, original_file_id = %meta.original_file_id, "Streaming Anthropic file content");

    // Stream the body directly back to the client, preserving upstream headers
    let body = axum::body::Body::from_stream(response.bytes_stream());
    let mut resp = axum::response::Response::new(body);
    *resp.status_mut() = status;
    *resp.headers_mut() = headers;
    resp
}
