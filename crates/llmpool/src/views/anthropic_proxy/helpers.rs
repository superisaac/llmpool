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

use crate::defer::OpenAIEventTask;

use super::client::{AnthropicUpstreamClient, build_anthropic_client};

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
        has_chat_completion: Some(true),
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

    let model_ids: Vec<i32> = models.iter().map(|(m, _)| m.id).collect();
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
    pub event_storage: RedisStorage<OpenAIEventTask>,
}

/// Check if the account has sufficient funds.
/// Returns Ok(()) if funds are sufficient, Err(Response) with a payment-required error otherwise.
pub async fn check_fund_balance(
    state: &AnthropicAppState,
    account_id: i32,
) -> Result<(), Response> {
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
