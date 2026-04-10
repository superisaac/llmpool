pub mod tasks;
pub mod worker;

use async_openai::{
    types::batches::{Batch, BatchRequest},
    types::chat::{
        CreateChatCompletionRequest, CreateChatCompletionResponse,
        CreateChatCompletionStreamResponse,
    },
    types::embeddings::{CreateEmbeddingRequest, CreateEmbeddingResponse},
    types::images::{CreateImageRequest, ImagesResponse},
    types::responses::{CreateResponse, Response},
};
use serde::{Deserialize, Serialize};

use apalis_redis::{RedisConfig, RedisStorage};
use redis::AsyncCommands;
use tracing::warn;

use crate::config;
use crate::views::anthropic_proxy::client::{
    CreateMessageBatchParams, CreateMessageParams, Message, MessageBatch,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "body")]
pub enum OpenAIEventData {
    CreateChatCompletionRequest(CreateChatCompletionRequest),
    CreateChatCompletionStreamResponse(CreateChatCompletionStreamResponse),
    CreateChatCompletionResponse(CreateChatCompletionResponse),
    CreateChatCompletionStreamResponseDone,

    CreateImageRequest(CreateImageRequest),
    ImagesResponse(ImagesResponse),

    CreateEmbeddingRequest(CreateEmbeddingRequest),
    CreateEmbeddingResponse(CreateEmbeddingResponse),

    BatchRequest(BatchRequest),
    Batch(Batch),

    ResponsesRequest(CreateResponse),
    ResponsesResponse(Response),
    ResponsesStreamDone,
}

/// An event entry to be processed asynchronously via the task queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIEventTask {
    pub session_id: String,
    pub session_index: i32,
    pub account_id: i64,
    pub model_id: i64,
    pub api_key_id: i64,
    pub event_data: OpenAIEventData,
}

/// Anthropic-specific event data variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "body")]
pub enum AnthropicEventData {
    /// Non-streaming message request
    CreateMessageRequest(CreateMessageParams),
    /// Non-streaming message response
    CreateMessageResponse(Message),
    /// Streaming message request
    CreateMessageStreamRequest(CreateMessageParams),
    /// Streaming message response done (carries final usage from message_delta)
    CreateMessageStreamResponseDone {
        input_tokens: u64,
        output_tokens: u64,
    },
    /// Batch message request
    CreateMessageBatchRequest(CreateMessageBatchParams),
    /// Batch message response
    CreateMessageBatchResponse(MessageBatch),
}

/// An Anthropic event entry to be processed asynchronously via the task queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicEventTask {
    pub session_id: String,
    pub session_index: i32,
    pub account_id: i64,
    pub model_id: i64,
    pub api_key_id: i64,
    pub event_data: AnthropicEventData,
}

/// A balance change entry to be processed asynchronously via the task queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceChangeTask {
    pub balance_change_id: i64,
}

/// Create a RedisStorage for EventEntry tasks by connecting to the configured Redis URL.
pub async fn create_event_storage() -> RedisStorage<OpenAIEventTask> {
    let redis_url = config::get_redis_url();
    let conn = apalis_redis::connect(redis_url)
        .await
        .expect("Failed to connect to Redis for task queue");
    RedisStorage::new(conn)
}

/// Create a RedisStorage for AnthropicEventTask tasks by connecting to the configured Redis URL.
pub async fn create_anthropic_event_storage() -> RedisStorage<AnthropicEventTask> {
    let redis_url = config::get_redis_url();
    let conn = apalis_redis::connect(redis_url)
        .await
        .expect("Failed to connect to Redis for anthropic event task queue");
    RedisStorage::new(conn)
}

/// Create a RedisStorage for BalanceChangeTask tasks by connecting to the configured Redis URL.
pub async fn create_balance_change_storage() -> RedisStorage<BalanceChangeTask> {
    let redis_url = config::get_redis_url();
    let conn = apalis_redis::connect(redis_url)
        .await
        .expect("Failed to connect to Redis for balance change task queue");
    RedisStorage::new(conn)
}

/// Remove stale worker entries from Redis to allow workers to re-register.
///
/// This is needed because apalis-redis's `register_worker.lua` script rejects
/// registration if a worker with the same name was seen within the keep-alive
/// threshold. When a worker crashes or is restarted quickly, the old entry
/// prevents the new worker from starting.
pub async fn cleanup_stale_workers(worker_names: &[&str]) {
    let redis_url = config::get_redis_url();
    let mut conn = apalis_redis::connect(redis_url)
        .await
        .expect("Failed to connect to Redis for worker cleanup");

    // Build configs matching what RedisStorage::new() would produce for each type
    let event_config =
        RedisConfig::default().set_namespace(std::any::type_name::<OpenAIEventTask>());
    let anthropic_event_config =
        RedisConfig::default().set_namespace(std::any::type_name::<AnthropicEventTask>());
    let balance_config =
        RedisConfig::default().set_namespace(std::any::type_name::<BalanceChangeTask>());

    let configs = [event_config, anthropic_event_config, balance_config];

    for config in &configs {
        let workers_set = config.workers_set();
        for worker_name in worker_names {
            let inflight_key = format!("{}:{}", config.inflight_jobs_set(), worker_name);
            let removed: Result<i64, _> = conn.zrem(&workers_set, &inflight_key).await;
            match removed {
                Ok(count) if count > 0 => {
                    warn!(
                        workers_set = %workers_set,
                        worker = %inflight_key,
                        "Removed stale worker entry from Redis"
                    );
                }
                Err(e) => {
                    warn!(
                        workers_set = %workers_set,
                        worker = %inflight_key,
                        error = %e,
                        "Failed to remove stale worker entry from Redis"
                    );
                }
                _ => {}
            }
        }
    }
}
