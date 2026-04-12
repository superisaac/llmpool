use axum::Router;

use crate::models::{LLMModel, LLMUpstream};
use crate::provider::types::{Provider, ProviderContext};

/// Provider implementation for Anthropic upstreams.
///
/// Handles all routes under `/anthropic/v1` and supports feature detection for
/// messages, files, and message batches.
pub struct AnthropicProvider;

impl Provider for AnthropicProvider {
    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn get_router_prefix(&self) -> &str {
        "/anthropic/v1"
    }

    fn get_router(&self, ctx: &ProviderContext) -> Router {
        crate::anthropic::proxy_views::get_router(
            ctx.pool.clone(),
            ctx.redis_pool.clone(),
            ctx.anthropic_event_storage.clone(),
            ctx.openai_event_storage.clone(),
        )
    }

    async fn detect_features(self, _model: &LLMModel, upstream: &LLMUpstream) -> Vec<String> {
        // 2. Delegate to the Anthropic feature detection logic
        crate::anthropic::features::detect_features(&upstream.api_key, &upstream.api_base).await
    }
}
