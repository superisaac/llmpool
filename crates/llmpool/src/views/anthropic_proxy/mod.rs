pub mod client;
pub mod helpers;
pub mod messages;

use axum::{Router, middleware, routing::post};
use std::sync::Arc;

use apalis_redis::RedisStorage;

use crate::db::{DbPool, RedisPool};
use crate::defer::OpenAIEventTask;
use crate::middlewares::api_auth::auth_anthropic_api;
use crate::views::openai_proxy::helpers::AppState as OpenAIAppState;

use helpers::AnthropicAppState;

pub fn get_router(
    pool: DbPool,
    redis_pool: RedisPool,
    event_storage: RedisStorage<OpenAIEventTask>,
) -> Router {
    // The Anthropic state (used by message handlers)
    let anthropic_state = Arc::new(AnthropicAppState {
        pool: pool.clone(),
        redis_pool: redis_pool.clone(),
        event_storage: event_storage.clone(),
    });

    // The OpenAI-compatible state used by the auth middleware
    let auth_state = Arc::new(OpenAIAppState {
        pool,
        redis_pool,
        event_storage,
    });

    Router::new()
        .route("/messages", post(messages::create_message))
        .route_layer(middleware::from_fn_with_state(
            auth_state,
            auth_anthropic_api,
        ))
        .with_state(anthropic_state)
}
