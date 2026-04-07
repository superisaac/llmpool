use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, warn};

use super::client::AnthropicUpstreamClient;
use super::helpers::{AnthropicAppState, check_fund_balance, select_anthropic_clients};
use crate::db;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};

// ---------------------------------------------------------------------------
// Anthropic Messages API request/response types
// ---------------------------------------------------------------------------

/// A single content block in a message (text, image, tool_use, tool_result, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlock {
    /// Simple string shorthand
    Text(String),
    /// Structured content block
    Block(Value),
}

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageParam {
    pub role: String,
    pub content: ContentBlock,
}

/// The full request body for POST /v1/messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    /// The model to use (e.g. "claude-3-5-sonnet-20241022")
    pub model: String,
    /// The conversation messages
    pub messages: Vec<MessageParam>,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Optional system prompt (string or array of content blocks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Sampling temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Top-p nucleus sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Top-k sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Tool definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    /// Tool choice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// POST /v1/messages — proxy to the configured Anthropic upstream
pub async fn create_message(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CreateMessageRequest>,
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
        match create_message_with_client(
            upstream_client,
            account_id,
            api_key_id,
            &payload,
            is_stream,
        )
        .await
        {
            Ok(response) => return response,
            Err(e) => {
                // Check if it's a network/connection error → mark upstream offline
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
// Internal helpers
// ---------------------------------------------------------------------------

/// Errors that can occur when calling the upstream Anthropic API
#[derive(Debug)]
pub enum AnthropicProxyError {
    /// A reqwest/network-level error
    Network(reqwest::Error),
    /// The upstream returned a non-2xx HTTP status
    Upstream { status: u16, body: String },
}

impl std::fmt::Display for AnthropicProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnthropicProxyError::Network(e) => write!(f, "network error: {}", e),
            AnthropicProxyError::Upstream { status, body } => {
                write!(f, "upstream error (HTTP {}): {}", status, body)
            }
        }
    }
}

fn is_network_error(e: &AnthropicProxyError) -> bool {
    matches!(e, AnthropicProxyError::Network(_))
}

/// Execute a single messages request against the given upstream client.
/// Returns Ok(Response) on success, Err(AnthropicProxyError) on failure.
async fn create_message_with_client(
    upstream: &AnthropicUpstreamClient,
    account_id: i32,
    api_key_id: i32,
    payload: &CreateMessageRequest,
    is_stream: bool,
) -> Result<Response, AnthropicProxyError> {
    let url = format!("{}/v1/messages", upstream.api_base);

    // Build the outgoing request body — always set stream explicitly
    let mut body = serde_json::to_value(payload).expect("payload serialization failed");
    body["stream"] = Value::Bool(is_stream);

    let req = upstream
        .http_client
        .post(&url)
        .header("x-api-key", &upstream.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body);

    let resp = req.send().await.map_err(AnthropicProxyError::Network)?;

    let status = resp.status();

    if !status.is_success() {
        let status_u16 = status.as_u16();
        let body_text = resp.text().await.unwrap_or_default();
        warn!(
            upstream_url = %url,
            http_status = status_u16,
            body = %body_text,
            "Anthropic upstream returned non-2xx response"
        );
        return Err(AnthropicProxyError::Upstream {
            status: status_u16,
            body: body_text,
        });
    }

    let model_db_id = upstream.model_db_id;

    if is_stream {
        // Stream the SSE response back to the client
        let byte_stream = resp.bytes_stream();
        let event_stream =
            transform_anthropic_stream(byte_stream, account_id, model_db_id, api_key_id);
        Ok(Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        // Parse the JSON response and forward it
        let response_body: Value = resp.json().await.map_err(AnthropicProxyError::Network)?;

        // Log token usage if present
        if let Some(usage) = response_body.get("usage") {
            info!(
                input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                output_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                model_db_id = model_db_id,
                "Anthropic message usage"
            );
        }

        Ok(Json(response_body).into_response())
    }
}

/// Transform the raw byte stream from the upstream Anthropic SSE response into
/// an Axum SSE event stream, forwarding each `data:` line as-is.
fn transform_anthropic_stream(
    byte_stream: impl Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send + 'static,
    account_id: i32,
    model_db_id: i32,
    _api_key_id: i32,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // We accumulate bytes into lines and forward SSE events.
    // Anthropic SSE format:
    //   event: message_start
    //   data: {...}
    //
    //   event: content_block_start
    //   data: {...}
    //   ...
    //   event: message_stop
    //   data: {...}

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
                                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
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
