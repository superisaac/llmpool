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

use anthropic_sdk::{MessageCreateParams, MessageStreamEvent};

use super::client::{
    CountMessageTokensParams, CountMessageTokensResponse, CreateMessageParams, Message, Usage,
};
use super::helpers::{
    AnthropicAppState, anthropic_sdk_request, check_wallet_balance, select_anthropic_clients,
};
use crate::anthropic::session_tracer::SessionTracer;
use crate::db;
use crate::defer::AnthropicEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};

// ---------------------------------------------------------------------------
// Conversion helpers: our proxy types → anthropic-sdk-rust types
// ---------------------------------------------------------------------------

/// Convert our proxy `CreateMessageParams` into the SDK's `MessageCreateParams`.
///
/// The SDK's `system` field is `Option<String>`, while ours is `Option<serde_json::Value>`
/// (which can be a string or an array of content blocks). We serialize non-string values
/// back to a JSON string so they are forwarded as-is.
fn to_sdk_params(params: &CreateMessageParams) -> MessageCreateParams {
    use anthropic_sdk::types::messages::{MessageContent, MessageParam, Role};

    // Convert messages
    let messages: Vec<MessageParam> = params
        .messages
        .iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "assistant" => Role::Assistant,
                _ => Role::User,
            };
            // ContentBlock can be a plain string or a structured Value
            let content = match &m.content {
                super::client::ContentBlock::Text(s) => MessageContent::Text(s.clone()),
                super::client::ContentBlock::Block(v) => {
                    // Serialize the block back to a JSON string for the SDK
                    MessageContent::Text(v.to_string())
                }
            };
            MessageParam { role, content }
        })
        .collect();

    // Convert system prompt: string → string, Value → JSON string
    let system = params.system.as_ref().map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    });

    MessageCreateParams {
        model: params.model.clone(),
        max_tokens: params.max_tokens,
        messages,
        system,
        temperature: params.temperature,
        top_p: params.top_p,
        top_k: params.top_k,
        stop_sequences: params.stop_sequences.clone(),
        stream: params.stream,
        tools: None, // tools are passed as serde_json::Value in our type; skip for now
        tool_choice: None,
        metadata: None, // metadata is serde_json::Value in our type; skip for now
    }
}

/// Convert the SDK's `Message` back into our proxy `Message` type so that
/// `AnthropicEventData` and JSON serialization remain unchanged.
fn from_sdk_message(sdk_msg: anthropic_sdk::Message) -> Message {
    use anthropic_sdk::ContentBlock as SdkContentBlock;

    let content: Vec<serde_json::Value> = sdk_msg
        .content
        .into_iter()
        .map(|block| match block {
            SdkContentBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            SdkContentBlock::Image { source } => {
                serde_json::json!({ "type": "image", "source": serde_json::to_value(source).unwrap_or_default() })
            }
            SdkContentBlock::ToolUse { id, name, input } => {
                serde_json::json!({ "type": "tool_use", "id": id, "name": name, "input": input })
            }
            SdkContentBlock::ToolResult { tool_use_id, content, is_error } => {
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error
                })
            }
        })
        .collect();

    let stop_reason = sdk_msg.stop_reason.map(|r| {
        use anthropic_sdk::StopReason;
        match r {
            StopReason::EndTurn => "end_turn".to_string(),
            StopReason::MaxTokens => "max_tokens".to_string(),
            StopReason::StopSequence => "stop_sequence".to_string(),
            StopReason::ToolUse => "tool_use".to_string(),
        }
    });

    let role_str = match sdk_msg.role {
        anthropic_sdk::Role::User => "user".to_string(),
        anthropic_sdk::Role::Assistant => "assistant".to_string(),
    };

    Message {
        id: sdk_msg.id,
        message_type: sdk_msg.type_,
        role: role_str,
        content,
        model: sdk_msg.model,
        stop_reason,
        stop_sequence: sdk_msg.stop_sequence,
        usage: Usage {
            input_tokens: sdk_msg.usage.input_tokens as u64,
            output_tokens: sdk_msg.usage.output_tokens as u64,
            cache_creation_input_tokens: sdk_msg
                .usage
                .cache_creation_input_tokens
                .map(|v| v as u64),
            cache_read_input_tokens: sdk_msg.usage.cache_read_input_tokens.map(|v| v as u64),
        },
    }
}

/// Determine whether an `anthropic_sdk::AnthropicError` is a network/connection error.
fn is_sdk_network_error(e: &anthropic_sdk::AnthropicError) -> bool {
    matches!(e, anthropic_sdk::AnthropicError::Connection { .. })
}

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

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
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
        let sdk_client = &upstream_client.client;
        let model_db_id = upstream_client.model_db_id;

        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            model_db_id,
            api_key_id,
        );

        // Override model with fullname for upstream request
        let mut upstream_payload = payload.clone();
        upstream_payload.model = upstream_client.fullname.clone();

        // Convert to SDK params
        let sdk_params = to_sdk_params(&upstream_payload);

        let result = if is_stream {
            // Record the request event
            tracer
                .add(AnthropicEventData::CreateMessageStreamRequest(
                    upstream_payload.clone(),
                ))
                .await;

            // Use SDK's create_stream for streaming
            match sdk_client.messages().create_stream(sdk_params).await {
                Ok(message_stream) => {
                    let event_stream = transform_sdk_stream(
                        message_stream,
                        account_id,
                        model_db_id,
                        api_key_id,
                        state.event_storage.clone(),
                        tracer,
                    );
                    Ok(Sse::new(event_stream)
                        .keep_alive(KeepAlive::default())
                        .into_response())
                }
                Err(e) => Err(e),
            }
        } else {
            // Record the request event
            tracer
                .add(AnthropicEventData::CreateMessageRequest(
                    upstream_payload.clone(),
                ))
                .await;

            match sdk_client.messages().create(sdk_params).await {
                Ok(sdk_message) => {
                    let message = from_sdk_message(sdk_message);
                    info!(
                        input_tokens = message.usage.input_tokens,
                        output_tokens = message.usage.output_tokens,
                        model_db_id = model_db_id,
                        "Anthropic message usage"
                    );
                    // Record the response event (with usage)
                    tracer
                        .add(AnthropicEventData::CreateMessageResponse(message.clone()))
                        .await;
                    Ok(Json(message).into_response())
                }
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(response) => return response,
            Err(e) => {
                if is_sdk_network_error(&e) {
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
// POST /v1/messages/count_tokens — Count tokens
// ---------------------------------------------------------------------------

/// POST /v1/messages/count_tokens — count tokens for a message without creating it
///
/// Uses the `anthropic-sdk-rust` client's underlying HTTP client to call
/// `POST /v1/messages/count_tokens` with proper authentication headers.
pub async fn count_message_tokens(
    State(state): State<Arc<AnthropicAppState>>,
    Json(payload): Json<CountMessageTokensParams>,
) -> Response {
    let model_name = payload.model.clone();
    let account_id = ACCOUNT.with(|u| u.id);

    if let Err(resp) = check_wallet_balance(&state, account_id).await {
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

    match anthropic_sdk_request::<CountMessageTokensParams, CountMessageTokensResponse>(
        &upstream_client.client,
        "/v1/messages/count_tokens",
        &payload,
    )
    .await
    {
        Ok(result) => {
            info!(
                input_tokens = result.input_tokens,
                model = %model_name,
                "Anthropic count_tokens result"
            );
            Json(result).into_response()
        }
        Err(e) => {
            if matches!(e, anthropic_sdk::AnthropicError::Connection { .. }) {
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
// Internal helpers — SDK streaming
// ---------------------------------------------------------------------------

/// Transform the `MessageStream` from `anthropic-sdk-rust` into an Axum SSE event stream.
///
/// Each `MessageStreamEvent` is serialised to JSON and forwarded as a `data:` line,
/// matching the SSE format that Anthropic clients expect.  Usage counters are
/// extracted from `MessageStart` and `MessageDelta` events and recorded via the
/// `SessionTracer` when the stream ends.
fn transform_sdk_stream(
    message_stream: anthropic_sdk::MessageStream,
    account_id: i64,
    model_db_id: i64,
    _api_key_id: i64,
    _event_storage: apalis_redis::RedisStorage<crate::defer::AnthropicEventTask>,
    mut tracer: SessionTracer,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let mut stream = message_stream;
        let mut stream_input_tokens: u64 = 0;
        let mut stream_output_tokens: u64 = 0;

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    // Track usage from relevant events
                    match &event {
                        MessageStreamEvent::MessageStart { message } => {
                            stream_input_tokens = message.usage.input_tokens as u64;
                        }
                        MessageStreamEvent::MessageDelta { usage, .. } => {
                            stream_output_tokens = usage.output_tokens as u64;
                            info!(
                                output_tokens = usage.output_tokens,
                                model_db_id = model_db_id,
                                account_id = account_id,
                                "Anthropic stream message_delta usage"
                            );
                        }
                        MessageStreamEvent::MessageStop => {
                            // Stream is done — record the final usage event
                            tracer.add(AnthropicEventData::CreateMessageStreamResponseDone {
                                input_tokens: stream_input_tokens,
                                output_tokens: stream_output_tokens,
                            }).await;
                        }
                        _ => {}
                    }

                    // Determine the SSE event type name and serialise the data payload
                    let (event_type, data_json) = sdk_event_to_sse(&event);
                    let mut sse_event = Event::default().data(data_json);
                    if let Some(et) = event_type {
                        sse_event = sse_event.event(et);
                    }
                    yield Ok(sse_event);
                }
                Err(e) => {
                    warn!(error = %e, "Error reading anthropic SDK stream event");
                    break;
                }
            }
        }
    }
}

/// Map a `MessageStreamEvent` to an `(event_type, data_json)` pair suitable for SSE.
fn sdk_event_to_sse(event: &MessageStreamEvent) -> (Option<String>, String) {
    let event_type = match event {
        MessageStreamEvent::MessageStart { .. } => Some("message_start".to_string()),
        MessageStreamEvent::MessageDelta { .. } => Some("message_delta".to_string()),
        MessageStreamEvent::MessageStop => Some("message_stop".to_string()),
        MessageStreamEvent::ContentBlockStart { .. } => Some("content_block_start".to_string()),
        MessageStreamEvent::ContentBlockDelta { .. } => Some("content_block_delta".to_string()),
        MessageStreamEvent::ContentBlockStop { .. } => Some("content_block_stop".to_string()),
    };

    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    (event_type, data)
}
