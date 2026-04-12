use async_openai::Client;
use axum::Router;

use super::proxy_views::helpers::build_client_from_upstream;
use crate::models::{LLMModel, LLMUpstream};
use crate::provider::types::{Provider, ProviderContext};

/// Provider implementation for OpenAI-compatible upstreams.
///
/// Handles all routes under `/openai/v1` and supports feature detection for
/// chat completions, images, embeddings, audio speech, and the responses API.
pub struct OpenAIProvider;

impl Provider for OpenAIProvider {
    fn provider_name(&self) -> &str {
        "openai"
    }

    fn get_router_prefix(&self) -> &str {
        "/openai/v1"
    }

    fn get_router(&self, ctx: &ProviderContext) -> Router {
        crate::openai::proxy_views::get_router(
            ctx.pool.clone(),
            ctx.redis_pool.clone(),
            ctx.openai_event_storage.clone(),
        )
    }

    async fn detect_features(self, model: &LLMModel, upstream: &LLMUpstream) -> Vec<String> {
        // 1. Build the OpenAI client
        let client = build_client_from_upstream(upstream);

        // 3. Build a minimal Model struct for feature detection
        let model_info = async_openai::types::models::Model {
            id: model.fullname.clone(),
            created: 0,
            object: "model".to_string(),
            owned_by: String::new(),
        };

        // 4. Delegate to the OpenAI feature detection logic
        crate::openai::features::detect_features(&client, &model_info).await
    }
}
