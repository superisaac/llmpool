use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
};
use futures::stream::{Stream, StreamExt};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, warn};

use super::client::{
    AnthropicApiError, CompletionRequest, CountMessageTokensParams, CreateMessageParams,
};
use super::helpers::{AnthropicAppState, check_fund_balance, select_anthropic_clients};
use crate::db;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};

// ---------------------------------------------------------------------------
// POST /v1/messages — Create a Message
// ---------------------------------------------------------------------------

/// POST /v1/messages — proxy to the configured Anthropic upstream
pub async fn create_message(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CreateMessageParams>,
) -> Response {
    let model_name = payload.model.clone();
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, &model_name, 2).await;
    if clients.is_empty() {
        warn!(model = %model_name, "No anthropic upstream client found for model");
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

    let api_key_id = API_CREDENTIAL.with(|k| k.id);
    let is_stream = payload.stream.unwrap_or(false);

    for (i, upstream_client) in clients.iter().enumerate() {
        let api_client = &upstream_client.client;
        let model_db_id = upstream_client.model_db_id;

        let result = if is_stream {
            // Use create_message_raw for streaming
            match api_client.create_message_raw(&payload).await {
                Ok(resp) => {
                    let byte_stream = resp.bytes_stream();
                    let event_stream = transform_anthropic_stream(
                        byte_stream,
                        account_id,
                        model_db_id,
                        api_key_id,
                    );
                    Ok(Sse::new(event_stream)
                        .keep_alive(KeepAlive::default())
                        .into_response())
                }
                Err(e) => Err(e),
            }
        } else {
            match api_client.create_message(&payload).await {
                Ok(message) => {
                    info!(
                        input_tokens = message.usage.input_tokens,
                        output_tokens = message.usage.output_tokens,
                        model_db_id = model_db_id,
                        "Anthropic message usage"
                    );
                    Ok(Json(message).into_response())
                }
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(response) => return response,
            Err(e) => {
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
                                "Failed to mark anthropic upstream as offline"
                            );
                        } else {
                            warn!(
                                upstream_id = upstream_id,
                                "Marked anthropic upstream as offline due to network error"
                            );
                        }
                    });
                }

                if i < clients.len() - 1 {
                    warn!(
                        model = %model_name,
                        error = %e,
                        "Anthropic message creation failed, retrying with another upstream"
                    );
                } else {
                    warn!(model = %model_name, error = %e, "Anthropic message creation failed after all retries");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "type": "error",
                            "error": {
                                "type": "api_error",
                                "message": "Internal server error while proxying to upstream."
                            }
                        })),
                    )
                        .into_response();
                }
            }
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// POST /v1/complete — Legacy Text Completions
// ---------------------------------------------------------------------------

/// POST /v1/complete — proxy legacy text completions to the configured Anthropic upstream
pub async fn create_completion(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CompletionRequest>,
) -> Response {
    let model_name = payload.model.clone();
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, &model_name, 2).await;
    if clients.is_empty() {
        warn!(model = %model_name, "No anthropic upstream client found for model (completion)");
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

    let is_stream = payload.stream.unwrap_or(false);
    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, upstream_client) in clients.iter().enumerate() {
        let api_client = &upstream_client.client;
        let model_db_id = upstream_client.model_db_id;

        let result = if is_stream {
            match api_client.create_completion_raw(&payload).await {
                Ok(resp) => {
                    let byte_stream = resp.bytes_stream();
                    let event_stream = transform_anthropic_stream(
                        byte_stream,
                        account_id,
                        model_db_id,
                        api_key_id,
                    );
                    Ok(Sse::new(event_stream)
                        .keep_alive(KeepAlive::default())
                        .into_response())
                }
                Err(e) => Err(e),
            }
        } else {
            match api_client.create_completion(&payload).await {
                Ok(completion) => {
                    info!(
                        model_db_id = model_db_id,
                        "Anthropic completion response received"
                    );
                    Ok(Json(completion).into_response())
                }
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(response) => return response,
            Err(e) => {
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
                                "Failed to mark anthropic upstream as offline"
                            );
                        } else {
                            warn!(
                                upstream_id = upstream_id,
                                "Marked anthropic upstream as offline due to network error"
                            );
                        }
                    });
                }

                if i < clients.len() - 1 {
                    warn!(
                        model = %model_name,
                        error = %e,
                        "Anthropic completion failed, retrying with another upstream"
                    );
                } else {
                    warn!(model = %model_name, error = %e, "Anthropic completion failed after all retries");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "type": "error",
                            "error": {
                                "type": "api_error",
                                "message": "Internal server error while proxying to upstream."
                            }
                        })),
                    )
                        .into_response();
                }
            }
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// POST /v1/messages/count_tokens — Count tokens
// ---------------------------------------------------------------------------

/// POST /v1/messages/count_tokens — count tokens for a message without creating it
pub async fn count_message_tokens(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CountMessageTokensParams>,
) -> Response {
    let model_name = payload.model.clone();
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_anthropic_clients(&state.pool, &state.redis_pool, &model_name, 1).await;
    if clients.is_empty() {
        warn!(model = %model_name, "No anthropic upstream client found for model (count_tokens)");
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

    match api_client.count_message_tokens(&payload).await {
        Ok(result) => {
            info!(
                input_tokens = result.input_tokens,
                model = %model_name,
                "Anthropic count_tokens result"
            );
            Json(result).into_response()
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
            warn!(model = %model_name, error = %e, "Anthropic count_tokens failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": "Internal server error while counting tokens."
                    }
                })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn is_network_error(e: &AnthropicApiError) -> bool {
    matches!(e, AnthropicApiError::Network(_))
}

/// Transform the raw byte stream from the upstream Anthropic SSE response into
/// an Axum SSE event stream, forwarding each `data:` line as-is.
fn transform_anthropic_stream(
    byte_stream: impl Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static,
    account_id: i32,
    model_db_id: i32,
    _api_key_id: i32,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut current_event_type: Option<String> = None;

        tokio::pin!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    let text = match std::str::from_utf8(&bytes) {
                        Ok(s) => s.to_string(),
                        Err(_) => continue,
                    };
                    buffer.push_str(&text);

                    // Process complete lines from the buffer
                    loop {
                        if let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                            buffer = buffer[newline_pos + 1..].to_string();

                            if line.is_empty() {
                                // Empty line = end of SSE event block; reset event type
                                current_event_type = None;
                                continue;
                            }

                            if let Some(event_type) = line.strip_prefix("event: ") {
                                current_event_type = Some(event_type.to_string());
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data: ") {
                                // Log usage from message_delta events
                                if current_event_type.as_deref() == Some("message_delta") {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                                        if let Some(usage) = parsed.get("usage") {
                                            info!(
                                                output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                                model_db_id = model_db_id,
                                                account_id = account_id,
                                                "Anthropic stream message_delta usage"
                                            );
                                        }
                                    }
                                }

                                // Build the SSE event, optionally with an event type
                                let mut sse_event = Event::default().data(data);
                                if let Some(ref et) = current_event_type {
                                    sse_event = sse_event.event(et.clone());
                                }
                                yield Ok(sse_event);
                            }
                        } else {
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Error reading anthropic stream chunk");
                    break;
                }
            }
        }
    }
}
