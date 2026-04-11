use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{CreateChatCompletionRequest, CreateChatCompletionStreamResponse},
};
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

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::db;
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::models::CapacityOption;
use crate::openai::session_tracer::SessionTracer;

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateChatCompletionRequest>,
) -> Response {
    let model_name = &payload.model;
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        feature: Some(crate::openai::features::FEATURE_CHAT_COMPLETIONS.to_string()),
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, upstream_client) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            upstream_client.model_db_id,
            api_key_id,
        );
        let mut upstream_payload = payload.clone();
        upstream_payload.model = upstream_client.fullname.clone();
        match chat_completions_with_client(&upstream_client.client, &mut tracer, upstream_payload)
            .await
        {
            Ok(response) => return response,
            Err(e) => {
                // On network errors, mark the upstream as offline
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
                        "Chat completion failed, retrying with another upstream"
                    );
                } else {
                    eprintln!("Chat completion failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Returns true if the OpenAI error is a network/reqwest error.
fn is_network_error(e: &async_openai::error::OpenAIError) -> bool {
    matches!(e, async_openai::error::OpenAIError::Reqwest(_))
}

/// Execute a chat completion request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn chat_completions_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateChatCompletionRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    let is_stream = payload.stream.unwrap_or(false);

    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateChatCompletionRequest(
            payload.clone(),
        ))
        .await;

    if is_stream {
        let stream = client.chat().create_stream(payload).await?;
        let tracer = tracer.clone();
        let event_stream = transform_stream_with_logging(stream, tracer);
        Ok(Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        let response = client.chat().create(payload).await?;
        if let Some(ref usage) = response.usage {
            info!(
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                total_tokens = usage.total_tokens,
                "Chat completion usage"
            );
        }

        // Log the response
        tracer
            .add(OpenAIEventData::CreateChatCompletionResponse(
                response.clone(),
            ))
            .await;

        Ok(Json(response).into_response())
    }
}

// Transform async-openai response stream into Axum SSE event stream with session logging
fn transform_stream_with_logging(
    mut stream: impl Stream<
        Item = Result<CreateChatCompletionStreamResponse, async_openai::error::OpenAIError>,
    > + Unpin
    + Send
    + 'static,
    mut tracer: SessionTracer,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    if let Some(ref usage) = response.usage {
                        info!(
                            prompt_tokens = usage.prompt_tokens,
                            completion_tokens = usage.completion_tokens,
                            total_tokens = usage.total_tokens,
                            "Chat completion stream usage"
                        );
                    }
                    // Log stream response chunk
                    tracer
                        .add(OpenAIEventData::CreateChatCompletionStreamResponse(response.clone()))
                        .await;

                    if let Ok(json_data) = serde_json::to_string(&response) {
                        yield Ok(Event::default().data(json_data));
                    }
                }
                Err(e) => {
                    eprintln!("Stream item error: {:?}", e);
                    // On error, we can choose to terminate or send an error event
                    break;
                }
            }
        }

        tracer
            .add(OpenAIEventData::CreateChatCompletionStreamResponseDone)
            .await;

        // Send the OpenAI-conventional end marker
        yield Ok(Event::default().data("[DONE]"));
    }
}
