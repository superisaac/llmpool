pub mod batches;
pub mod chat_completions;
pub mod embeddings;
pub mod files;
pub mod helpers;
pub mod images;
pub mod models;
pub mod responses;
pub mod speechs;

use apalis_redis::RedisStorage;
use axum::{
    Router, middleware,
    routing::{get, post},
};
use std::sync::Arc;

use crate::db::{DbPool, RedisPool};
use crate::defer::OpenAIEventTask;
use crate::middlewares::api_auth::auth_openai_api;

pub fn get_router(
    pool: DbPool,
    redis_pool: RedisPool,
    event_storage: RedisStorage<OpenAIEventTask>,
) -> Router {
    let state = Arc::new(helpers::AppState {
        pool,
        redis_pool,
        event_storage,
    });
    Router::new()
        .route("/models", get(models::list_merged_models))
        .route(
            "/chat/completions",
            post(chat_completions::chat_completions),
        )
        .route("/embeddings", post(embeddings::create_embeddings))
        .route("/images/generations", post(images::generate_images))
        // Audio-related routes
        .route("/audio/speech", post(speechs::create_speech))
        // Files routes
        .route("/files", post(files::create_file_handler))
        .route(
            "/files/{file_id}",
            get(files::retrieve_file_handler).delete(files::delete_file_handler),
        )
        .route("/files/{file_id}/content", get(files::file_content_handler))
        // Batches routes
        .route("/batches", post(batches::create_batch_handler))
        .route("/batches/{batch_id}", get(batches::batch_by_id_handler))
        .route(
            "/batches/{batch_id}/cancel",
            post(batches::batch_cancel_handler),
        )
        // Responses routes
        .route("/responses", post(responses::create_response))
        .route(
            "/responses/{response_id}",
            get(responses::retrieve_response),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_openai_api,
        ))
        .with_state(state)
}
