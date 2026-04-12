use axum::Router;

use apalis_redis::RedisStorage;

use crate::db::{DbPool, RedisPool};
use crate::defer::{AnthropicEventTask, OpenAIEventTask};
use crate::models::{LLMModel, LLMUpstream};

/// Runtime dependencies required to build a provider's Axum router.
///
/// Both providers need a database pool and a Redis pool; each provider also
/// needs its own event-task storage queue.
pub struct ProviderContext {
    pub pool: DbPool,
    pub redis_pool: RedisPool,
    pub openai_event_storage: RedisStorage<OpenAIEventTask>,
    pub anthropic_event_storage: RedisStorage<AnthropicEventTask>,
}

/// Abstract interface for an API provider (e.g. OpenAI, Anthropic).
///
/// Each provider knows:
/// - its canonical name (used as the `provider` field in the database)
/// - the URL prefix under which its proxy routes are mounted
/// - how to build its Axum router given a [`ProviderContext`]
/// - which feature strings a given model supports (static, name-based detection)
pub trait Provider: Send + Sync {
    /// The canonical provider name stored in the database (e.g. `"openai"`, `"anthropic"`).
    fn provider_name(&self) -> &str;

    /// The URL path prefix under which this provider's routes are mounted
    /// (e.g. `"/openai/v1"`, `"/anthropic/v1"`).
    fn get_router_prefix(&self) -> &str;

    /// Build and return the Axum [`Router`] for this provider using the supplied runtime context.
    fn get_router(&self, ctx: &ProviderContext) -> Router;

    /// Detect which features are supported by the given model.
    ///
    /// Fetches the upstream associated with `model`, builds the provider-specific
    /// client, and delegates to the concrete provider's feature-detection logic.
    /// Returns a list of supported feature identifier strings.
    #[allow(async_fn_in_trait)]
    async fn detect_features(self, model: &LLMModel, upstream: &LLMUpstream) -> Vec<String>
    where
        Self: Sized;
}

/// Look up a provider by its canonical name.
///
/// Returns `None` when no provider with that name is registered.
pub fn get_provider(provider_name: &str) -> Option<Box<dyn Provider>> {
    get_all_providers()
        .into_iter()
        .find(|p| p.provider_name() == provider_name)
}

/// Return every registered provider.
pub fn get_all_providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(crate::openai::provider::OpenAIProvider),
        Box::new(crate::anthropic::provider::AnthropicProvider),
    ]
}
