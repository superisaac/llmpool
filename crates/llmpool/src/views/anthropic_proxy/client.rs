use crate::models::{LLMModel, LLMUpstream};

// --- Upstream client for Anthropic ---

pub struct AnthropicUpstreamClient {
    /// The reqwest HTTP client (possibly configured with a proxy)
    pub http_client: reqwest::Client,
    /// The upstream API base URL (e.g. "https://api.anthropic.com")
    pub api_base: String,
    /// The decrypted API key
    pub api_key: String,
    /// The LLMModel primary key
    pub model_db_id: i32,
    /// The LLMUpstream primary key (used to mark upstream offline on network errors)
    pub upstream_id: i32,
}

/// Build an `AnthropicUpstreamClient` from a (LLMModel, LLMUpstream) pair.
/// If the upstream has proxies configured, a random one is selected and used.
pub fn build_anthropic_client(model: &LLMModel, upstream: &LLMUpstream) -> AnthropicUpstreamClient {
    use rand::seq::IndexedRandom;

    let http_client = if !upstream.proxies.is_empty() {
        let mut rng = rand::rng();
        if let Some(proxy_url) = upstream.proxies.choose(&mut rng) {
            tracing::info!(
                upstream_name = %upstream.name,
                proxy = %proxy_url,
                "Anthropic proxy: using proxy for upstream"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy")
        } else {
            reqwest::Client::new()
        }
    } else {
        reqwest::Client::new()
    };

    AnthropicUpstreamClient {
        http_client,
        api_base: upstream.api_base.trim_end_matches('/').to_string(),
        api_key: upstream.api_key.clone(),
        model_db_id: model.id,
        upstream_id: upstream.id,
    }
}
