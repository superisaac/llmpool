use anthropic_sdk::{Anthropic, ClientConfig};
use apalis_redis::RedisStorage;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::warn;

use crate::db;
use crate::db::{DbPool, RedisPool};
use crate::models::{CapacityOption, LLMModel, LLMUpstream};

use crate::defer::AnthropicEventTask;

// --- Upstream client for Anthropic ---

pub struct AnthropicUpstreamClient {
    /// The Anthropic SDK client
    pub client: Anthropic,
    /// The LLMModel primary key
    pub model_db_id: i64,
    /// The LLMUpstream primary key (used to mark upstream offline on network errors)
    pub upstream_id: i64,
    /// The full model identifier to use when sending requests to the upstream
    pub fullname: String,
}

/// Build an `AnthropicUpstreamClient` from a (LLMModel, LLMUpstream) pair.
/// Uses `anthropic-sdk-rust`'s `Anthropic` client with `ClientConfig`.
/// If the upstream has proxies configured, a random one is selected and used via
/// `ClientConfig::with_proxy_url()`.
pub fn build_anthropic_client(model: &LLMModel, upstream: &LLMUpstream) -> AnthropicUpstreamClient {
    use rand::seq::IndexedRandom;

    // Build the ClientConfig with the upstream's API key and base URL
    let mut config = ClientConfig::new(upstream.api_key.clone())
        .with_base_url(upstream.api_base.trim_end_matches('/').to_string());

    // If proxies are configured, pick a random one and pass it to the SDK
    if !upstream.proxies.is_empty() {
        let mut rng = rand::rng();
        if let Some(proxy_url) = upstream.proxies.choose(&mut rng) {
            tracing::info!(
                upstream_name = %upstream.name,
                proxy = %proxy_url,
                "Anthropic proxy: using proxy for upstream"
            );
            config = config.with_proxy_url(proxy_url.clone());
        }
    }

    let client = Anthropic::with_config(config).expect("Failed to build Anthropic SDK client");

    AnthropicUpstreamClient {
        client,
        model_db_id: model.id,
        upstream_id: upstream.id,
        fullname: model.fullname.clone(),
    }
}

/// Returns up to `count` AnthropicUpstreamClient entries selected by lowest current-hour output
/// token usage from Redis. If a model has no Redis key, its usage defaults to 0.
pub async fn select_anthropic_clients(
    db_pool: &DbPool,
    redis_pool: &RedisPool,
    model_name: &str,
    count: usize,
) -> Vec<AnthropicUpstreamClient> {
    use crate::redis_utils::counters::get_output_token_usage_batch;

    let capacity = CapacityOption {
        has_messages: Some(true),
        ..Default::default()
    };

    let models =
        match db::llm::find_models_by_name_and_capacity(db_pool, model_name, &capacity).await {
            Ok(models) if !models.is_empty() => models,
            Ok(_) => {
                warn!(
                    model = model_name,
                    "No models found in DB for the requested capacity (anthropic)"
                );
                return vec![];
            }
            Err(e) => {
                warn!(
                    model = model_name,
                    error = %e,
                    "DB query failed when looking up models (anthropic)"
                );
                return vec![];
            }
        };

    let model_ids: Vec<i64> = models.iter().map(|(m, _)| m.id).collect();
    let usages = get_output_token_usage_batch(redis_pool, &model_ids).await;

    let mut models_with_usage: Vec<(i64, &(LLMModel, LLMUpstream))> =
        usages.into_iter().zip(models.iter()).collect();

    models_with_usage.sort_by_key(|(usage, _)| *usage);
    models_with_usage.truncate(count);

    models_with_usage
        .into_iter()
        .map(|(usage, (model, upstream))| {
            tracing::info!(
                model = model_name,
                upstream_name = upstream.name,
                api_base = upstream.api_base,
                output_token_usage = usage,
                "Selected anthropic upstream candidate by lowest output token usage"
            );
            build_anthropic_client(model, upstream)
        })
        .collect()
}

// --- Server State ---

pub struct AnthropicAppState {
    pub pool: DbPool,
    pub redis_pool: RedisPool,
    pub event_storage: RedisStorage<AnthropicEventTask>,
}

/// Check if the account has sufficient wallets.
/// Returns Ok(()) if wallets are sufficient, Err(Response) with a payment-required error otherwise.
pub async fn check_wallet_balance(
    state: &AnthropicAppState,
    account_id: i64,
) -> Result<(), Response> {
    use crate::redis_utils::caches::wallet as wallet_cache;
    use bigdecimal::BigDecimal;

    let wallet = match wallet_cache::get_wallet_info(&state.redis_pool, account_id).await {
        Ok(Some(f)) => f,
        _ => match db::wallet::find_account_wallet(&state.pool, account_id).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                return Err((
                    StatusCode::PAYMENT_REQUIRED,
                    Json(serde_json::json!({
                        "error": {
                            "message": "No wallet record found for this account.",
                            "type": "insufficient_wallets",
                            "code": "no_wallet_record"
                        }
                    })),
                )
                    .into_response());
            }
            Err(e) => {
                warn!(error = %e, "Database error during wallet lookup");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": {
                            "message": "Internal server error during wallet lookup.",
                            "type": "server_error",
                            "code": "internal_error"
                        }
                    })),
                )
                    .into_response());
            }
        },
    };

    if wallet.balance.clone() <= BigDecimal::from(0) {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            Json(serde_json::json!({
                "error": {
                    "message": "Balance is insufficient, please fund your wallet",
                    "type": "insufficient_wallets",
                    "code": "insufficient_wallets"
                }
            })),
        )
            .into_response());
    }

    Ok(())
}
