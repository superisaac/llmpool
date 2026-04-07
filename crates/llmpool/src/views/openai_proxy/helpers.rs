use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::{info, warn};

use apalis_redis::RedisStorage;

use crate::db::{self, DbPool, RedisPool};
use crate::defer::OpenAIEventTask;

// --- Server State ---

pub struct AppState {
    pub pool: DbPool,
    pub redis_pool: RedisPool,
    pub event_storage: RedisStorage<OpenAIEventTask>,
}

// --- Helpers ---

/// Check if the account has sufficient funds (cash + credit > 0).
/// Tries Redis cache first, falls back to DB on cache miss.
/// Returns Ok(()) if funds are sufficient, Err(Response) with a payment-required error otherwise.
pub async fn check_fund_balance(state: &AppState, account_id: i32) -> Result<(), Response> {
    use crate::redis_utils::caches::fund as fund_cache;
    use bigdecimal::BigDecimal;

    let fund = match fund_cache::get_fund_info(&state.redis_pool, account_id).await {
        Ok(Some(f)) => f,
        _ => match db::fund::find_account_fund(&state.pool, account_id).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                return Err((
                    StatusCode::PAYMENT_REQUIRED,
                    Json(serde_json::json!({
                        "error": {
                            "message": "No fund record found for this account.",
                            "type": "insufficient_funds",
                            "code": "no_fund_record"
                        }
                    })),
                )
                    .into_response());
            }
            Err(e) => {
                warn!(error = %e, "Database error during fund lookup");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": {
                            "message": "Internal server error during fund lookup.",
                            "type": "server_error",
                            "code": "internal_error"
                        }
                    })),
                )
                    .into_response());
            }
        },
    };

    if fund.balance.clone() <= BigDecimal::from(0) {
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            Json(serde_json::json!({
                "error": {
                    "message": "账户余额不够，请充值后继续使用。",
                    "type": "insufficient_funds",
                    "code": "insufficient_funds"
                }
            })),
        )
            .into_response());
    }

    Ok(())
}

/// Select the first available upstream from the database.
/// Returns an error Response if no upstream is found.
pub(super) async fn select_first_upstream(
    state: &AppState,
) -> Result<crate::models::LLMUpstream, Response> {
    match db::llm::list_upstreams(&state.pool).await {
        Ok(upstreams) if !upstreams.is_empty() => Ok(upstreams.into_iter().next().unwrap()),
        Ok(_) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": {
                    "message": "No upstream upstreams configured.",
                    "type": "server_error",
                    "code": "no_upstream"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(error = %e, "Failed to query upstreams for files/batches proxy");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "Internal server error while selecting upstream.",
                        "type": "server_error",
                        "code": "internal_error"
                    }
                })),
            )
                .into_response())
        }
    }
}

/// Build an async-openai Client from an LLMUpstream (no model_id needed).
pub(super) fn build_client_from_upstream(
    upstream: &crate::models::LLMUpstream,
) -> async_openai::Client<async_openai::config::OpenAIConfig> {
    use async_openai::{Client, config::OpenAIConfig};

    let config = OpenAIConfig::new()
        .with_api_key(upstream.api_key.clone())
        .with_api_base(upstream.api_base.clone());

    if !upstream.proxies.is_empty() {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if let Some(proxy_url) = upstream.proxies.choose(&mut rng) {
            info!(
                upstream_name = %upstream.name,
                proxy = %proxy_url,
                "OpenAI proxy: using proxy for upstream (batches)"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            let http_client = reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy");
            return Client::with_config(config).with_http_client(http_client);
        }
    }
    Client::with_config(config)
}

/// Build a Client from an (LLMModel, LLMUpstream) pair.
/// If the upstream has proxies configured, a random one is selected and used.
pub(super) fn build_client_from_model_upstream(
    model: &crate::models::LLMModel,
    upstream: &crate::models::LLMUpstream,
) -> (
    async_openai::Client<async_openai::config::OpenAIConfig>,
    i32,
) {
    use async_openai::{Client, config::OpenAIConfig};

    let config = OpenAIConfig::new()
        .with_api_key(upstream.api_key.clone())
        .with_api_base(upstream.api_base.clone());

    let client = if !upstream.proxies.is_empty() {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if let Some(proxy_url) = upstream.proxies.choose(&mut rng) {
            info!(
                upstream_name = %upstream.name,
                proxy = %proxy_url,
                "OpenAI proxy: using proxy for upstream"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            let http_client = reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy");
            Client::with_config(config).with_http_client(http_client)
        } else {
            Client::with_config(config)
        }
    } else {
        Client::with_config(config)
    };

    (client, model.id)
}

/// A selected upstream client with associated IDs for tracking and error handling.
pub(super) struct UpstreamClient {
    pub client: async_openai::Client<async_openai::config::OpenAIConfig>,
    /// The LLMModel primary key
    pub model_db_id: i32,
    /// The LLMUpstream primary key (used to mark upstream offline on network errors)
    pub upstream_id: i32,
}

/// Returns up to `count` UpstreamClient entries selected by lowest current-hour output
/// token usage from Redis. If a model has no Redis key, its usage defaults to 0.
pub(super) async fn select_model_clients(
    db_pool: &DbPool,
    redis_pool: &RedisPool,
    model_name: &str,
    capacity: &crate::models::CapacityOption,
    count: usize,
) -> Vec<UpstreamClient> {
    use crate::redis_utils::counters::get_output_token_usage_batch;

    let models = match db::llm::find_models_by_name_and_capacity(
        db_pool, model_name, capacity, None,
    )
    .await
    {
        Ok(models) if !models.is_empty() => models,
        Ok(_) => {
            warn!(
                model = model_name,
                "No models found in DB for the requested capacity"
            );
            return vec![];
        }
        Err(e) => {
            warn!(
                model = model_name,
                error = %e,
                "DB query failed when looking up models"
            );
            return vec![];
        }
    };

    // Fetch output token usage from Redis for all models in a single MGET, then sort ascending
    // and take the `count` with the lowest usage.
    let model_ids: Vec<i32> = models.iter().map(|(m, _)| m.id).collect();
    let usages = get_output_token_usage_batch(redis_pool, &model_ids).await;

    let mut models_with_usage: Vec<(i64, &(crate::models::LLMModel, crate::models::LLMUpstream))> =
        usages.into_iter().zip(models.iter()).collect();

    models_with_usage.sort_by_key(|(usage, _)| *usage);
    models_with_usage.truncate(count);

    models_with_usage
        .into_iter()
        .map(|(usage, (model, upstream))| {
            info!(
                model = model_name,
                upstream_name = upstream.name,
                api_base = upstream.api_base,
                output_token_usage = usage,
                "Selected upstream candidate by lowest output token usage"
            );
            let (client, model_db_id) = build_client_from_model_upstream(model, upstream);
            UpstreamClient {
                client,
                model_db_id,
                upstream_id: upstream.id,
            }
        })
        .collect()
}
