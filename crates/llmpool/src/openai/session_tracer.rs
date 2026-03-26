use apalis::prelude::*;
use apalis_redis::RedisStorage;
use tracing::warn;
use uuid::Uuid;

use crate::defer::{OpenAIEventData, OpenAIEventTask};

#[derive(Clone)]
pub struct SessionTracer {
    pub session_id: String,
    pub session_index: i32,
    pub user_id: i32,
    pub model_id: i32,
    pub storage: RedisStorage<OpenAIEventTask>,
}

impl SessionTracer {
    /// Create a new SessionTracer with a UUIDv7-based session_id
    pub fn new(storage: RedisStorage<OpenAIEventTask>, user_id: i32, model_id: i32) -> Self {
        let session_id = Uuid::now_v7().to_string();
        let session_index = 0;
        Self {
            session_id,
            session_index,
            user_id,
            model_id,
            storage,
        }
    }

    /// Add a session event by enqueuing it to the async task queue.
    ///
    /// The actual database operations (creating session events, balance changes, etc.)
    /// are handled by the deferred worker via `handle_event`.
    pub async fn add(&mut self, data: OpenAIEventData) {
        let session_index = self.session_index;
        self.session_index += 1;
        let entry = OpenAIEventTask {
            session_id: self.session_id.clone(),
            session_index,
            user_id: self.user_id,
            model_id: self.model_id,
            event_data: data,
        };

        if let Err(e) = self.storage.push(entry).await {
            warn!(
                error = %e,
                session_id = %self.session_id,
                "Failed to enqueue openai event to task queue"
            );
        }
    }
}
