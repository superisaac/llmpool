use axum::Router;

use crate::anthropic::proxy_views::helpers::build_anthropic_client_context;
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

    fn detect_features<'a>(
        &'a self,
        model: &'a LLMModel,
        upstream: &'a LLMUpstream,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<String>> + Send + 'a>> {
        Box::pin(async move {
            let ctx = build_anthropic_client_context(model, upstream);
            crate::anthropic::features::detect_features(&ctx.client, model).await
        })
    }
}
