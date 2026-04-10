use apalis::prelude::*;
use apalis_redis::RedisStorage;
use tracing::warn;
use uuid::Uuid;

use crate::defer::{AnthropicEventData, AnthropicEventTask};

#[derive(Clone)]
pub struct SessionTracer {
    pub session_id: String,
    pub session_index: i32,
    pub account_id: i64,
    pub model_id: i64,
    pub api_key_id: i64,
    pub storage: RedisStorage<AnthropicEventTask>,
}

impl SessionTracer {
    /// Create a new SessionTracer with a UUIDv7-based session_id
    pub fn new(
        storage: RedisStorage<AnthropicEventTask>,
        account_id: i64,
        model_id: i64,
        api_key_id: i64,
    ) -> Self {
        let session_id = Uuid::now_v7().to_string();
        let session_index = 0;
        Self {
            session_id,
            session_index,
            account_id,
            model_id,
            api_key_id,
            storage,
        }
    }

    /// Add a session event by enqueuing it to the async task queue.
    ///
    /// The actual database operations (creating session events, balance changes, etc.)
    /// are handled by the deferred worker via `handle_anthropic_event`.
    pub async fn add(&mut self, data: AnthropicEventData) {
        let session_index = self.session_index;
        self.session_index += 1;
        let entry = AnthropicEventTask {
            session_id: self.session_id.clone(),
            session_index,
            account_id: self.account_id,
            model_id: self.model_id,
            api_key_id: self.api_key_id,
            event_data: data,
        };

        if let Err(e) = self.storage.push(entry).await {
            warn!(
                error = %e,
                session_id = %self.session_id,
                "Failed to enqueue anthropic event to task queue"
            );
        }
    }
}
