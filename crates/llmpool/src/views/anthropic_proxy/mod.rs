pub mod batches;
pub mod client;
pub mod helpers;
pub mod messages;

use axum::{Router, middleware, routing::post};
use std::sync::Arc;

use apalis_redis::RedisStorage;

use crate::db::{DbPool, RedisPool};
use crate::defer::{AnthropicEventTask, OpenAIEventTask};
use crate::middlewares::api_auth::auth_anthropic_api;
use crate::views::openai_proxy::helpers::AppState as OpenAIAppState;

use helpers::AnthropicAppState;

pub fn get_router(
    pool: DbPool,
    redis_pool: RedisPool,
    anthropic_event_storage: RedisStorage<AnthropicEventTask>,
    openai_event_storage: RedisStorage<OpenAIEventTask>,
) -> Router {
    // The Anthropic state (used by message handlers)
    let anthropic_state = Arc::new(AnthropicAppState {
        pool: pool.clone(),
        redis_pool: redis_pool.clone(),
        event_storage: anthropic_event_storage,
    });

    // The OpenAI-compatible state used by the auth middleware
    let auth_state = Arc::new(OpenAIAppState {
        pool,
        redis_pool,
        event_storage: openai_event_storage,
    });

    Router::new()
        // POST /v1/messages — Create a Message
        .route("/messages", post(messages::create_message))
        // POST /v1/complete — Legacy Text Completions
        .route("/complete", post(messages::create_completion))
        // POST /v1/messages/count_tokens — Count tokens
        .route(
            "/messages/count_tokens",
            post(messages::count_message_tokens),
        )
        // POST /v1/messages/batches — Create a Message Batch
        // GET  /v1/messages/batches — List Message Batches
        // .route(
        //     "/messages/batches",
        //     post(batches::create_message_batch).get(batches::list_message_batches),
        // )
        // // GET  /v1/messages/batches/:id — Retrieve a Message Batch
        // .route(
        //     "/messages/batches/:message_batch_id",
        //     get(batches::retrieve_message_batch),
        // )
        // // POST /v1/messages/batches/:id/cancel — Cancel a Message Batch
        // .route(
        //     "/messages/batches/:message_batch_id/cancel",
        //     post(batches::cancel_message_batch),
        // )
        // // GET  /v1/messages/batches/:id/results — Retrieve Batch Results
        // .route(
        //     "/messages/batches/:message_batch_id/results",
        //     get(batches::retrieve_message_batch_results),
        // )
        .route_layer(middleware::from_fn_with_state(
            auth_state,
            auth_anthropic_api,
        ))
        .with_state(anthropic_state)
}
