use apalis::prelude::*;
use apalis_redis::RedisStorage;
use bigdecimal::BigDecimal;
use chrono::Utc;
use redis::AsyncCommands;
use tracing::{info, warn};

use crate::config;
use crate::db::{self, DbPool};
use crate::defer::{BalanceChangeTask, OpenAIEventData, OpenAIEventTask};
use crate::models::{
    BalanceChangeContent, NewBalanceChange, NewSessionEvent, LLMModel, SpendToken,
};

/// Represents extracted usage information from a response
struct UsageInfo {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

/// Extract usage information from a OpenAIEventData if available
fn extract_usage(data: &OpenAIEventData) -> Option<UsageInfo> {
    match data {
        OpenAIEventData::CreateChatCompletionResponse(resp) => {
            resp.usage.as_ref().map(|u| UsageInfo {
                input_tokens: u.prompt_tokens as i64,
                output_tokens: u.completion_tokens as i64,
                total_tokens: u.total_tokens as i64,
            })
        }
        OpenAIEventData::CreateChatCompletionStreamResponse(resp) => {
            resp.usage.as_ref().map(|u| UsageInfo {
                input_tokens: u.prompt_tokens as i64,
                output_tokens: u.completion_tokens as i64,
                total_tokens: u.total_tokens as i64,
            })
        }
        OpenAIEventData::CreateEmbeddingResponse(resp) => Some(UsageInfo {
            input_tokens: resp.usage.prompt_tokens as i64,
            output_tokens: 0,
            total_tokens: resp.usage.total_tokens as i64,
        }),
        OpenAIEventData::ImagesResponse(resp) => resp.usage.as_ref().map(|u| UsageInfo {
            input_tokens: u.input_tokens as i64,
            output_tokens: u.output_tokens as i64,
            total_tokens: u.total_tokens as i64,
        }),
        // Request types and stream done marker don't have usage
        _ => None,
    }
}

/// Increment the hourly token usage counters in Redis for the given model.
///
/// Keys follow the pattern:
///   `tokenusage:<model_id>:<hour>.input`
///   `tokenusage:<model_id>:<hour>.output`
///
/// where `<hour>` is formatted as `YYYYMMDDHH` (UTC).
async fn increment_token_usage(model_id: i32, input_tokens: i64, output_tokens: i64) {
    let redis_url = config::get_redis_url();
    let client = match redis::Client::open(redis_url) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to create Redis client for token usage counter");
            return;
        }
    };
    let mut conn = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to connect to Redis for token usage counter");
            return;
        }
    };

    // Format the current UTC hour as YYYYMMDDHH
    let hour = Utc::now().format("%Y%m%d%H").to_string();
    let input_key = format!("tokenusage:input:{}:{}", model_id, hour);
    let output_key = format!("tokenusage:output:{}:{}", model_id, hour);

    if input_tokens > 0 {
        if let Err(e) = conn.incr::<_, i64, i64>(&input_key, input_tokens).await {
            warn!(error = %e, key = %input_key, "Failed to increment input token usage in Redis");
        } else if let Err(e) = conn.expire::<_, bool>(&input_key, 3600).await {
            warn!(error = %e, key = %input_key, "Failed to set TTL on input token usage key in Redis");
        }
    }
    if output_tokens > 0 {
        if let Err(e) = conn.incr::<_, i64, i64>(&output_key, output_tokens).await {
            warn!(error = %e, key = %output_key, "Failed to increment output token usage in Redis");
        } else if let Err(e) = conn.expire::<_, bool>(&output_key, 3600).await {
            warn!(error = %e, key = %output_key, "Failed to set TTL on output token usage key in Redis");
        }
    }
}

/// Handle an event entry from the async task queue.
///
/// This performs the following:
/// 1. Create a session event record in the database
/// 2. If the event contains usage info, look up model token prices and create a balance change
/// 3. Enqueue a settle_balance_change task to apply the balance change asynchronously
///
/// Database operations for session event and balance change creation are executed within a single transaction.
pub async fn handle_openai_event(
    event: OpenAIEventTask,
    pool: Data<DbPool>,
    balance_change_storage: Data<RedisStorage<BalanceChangeTask>>,
) {
    let session_id = event.session_id.clone();
    let consumer_id = event.consumer_id;
    let model_id = event.model_id;

    info!(
        session_id = %session_id,
        session_index = event.session_index,
        consumer_id = consumer_id,
        model_id = model_id,
        "Processing deferred event"
    );

    // Extract usage before serializing
    let usage = extract_usage(&event.event_data);

    // Increment hourly token usage counters in Redis
    if let Some(u) = &usage {
        increment_token_usage(model_id, u.input_tokens, u.output_tokens).await;
    }

    let event_data_json =
        serde_json::to_value(&event.event_data).unwrap_or(serde_json::Value::Null);

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            warn!(
                error = %e,
                session_id = %session_id,
                "Failed to begin database transaction"
            );
            return;
        }
    };

    // Look up the model to get token prices (needed for both session event and balance change)
    let model: Option<LLMModel> = match db::openai::get_model_with_tx(&mut tx, model_id).await {
        Ok(model) => Some(model),
        Err(e) => {
            warn!(
                error = %e,
                session_id = %session_id,
                "Failed to look up model, using default token prices"
            );
            None
        }
    };

    // Build the new session event with token price and usage info
    let (input_token_price, output_token_price) = match &model {
        Some(m) => (m.input_token_price.clone(), m.output_token_price.clone()),
        None => (BigDecimal::from(0), BigDecimal::from(0)),
    };
    let (input_tokens, output_tokens) = match &usage {
        Some(u) => (u.input_tokens, u.output_tokens),
        None => (0, 0),
    };

    let new_event = NewSessionEvent {
        session_id: session_id.clone(),
        session_index: event.session_index,
        consumer_id,
        model_id,
        api_key_id: event.api_key_id,
        input_token_price: input_token_price.clone(),
        input_tokens,
        output_token_price: output_token_price.clone(),
        output_tokens,
        event_data: event_data_json,
    };

    // 1. Create the session event
    let event = match db::session_event::create_session_event_with_tx(&mut tx, &new_event).await {
        Ok(event) => event,
        Err(e) => {
            warn!(
                error = %e,
                session_id = %session_id,
                "Failed to create session event"
            );
            return;
        }
    };

    // 2. If there's usage info and model is available, create a balance change
    if let (Some(usage), Some(model)) = (usage, model) {
        let input_spend_amount = &model.input_token_price * BigDecimal::from(usage.input_tokens);
        let output_spend_amount = &model.output_token_price * BigDecimal::from(usage.output_tokens);

        let spend_token = SpendToken {
            input_tokens: usage.input_tokens,
            input_token_price: model.input_token_price.clone(),
            input_spend_amount,
            output_tokens: usage.output_tokens,
            output_token_price: model.output_token_price.clone(),
            output_spend_amount,
            total_tokens: usage.total_tokens,
            event_id: event.id,
        };

        let content = BalanceChangeContent::SpendToken(spend_token);
        let unique_request_id = format!("spendtoken-{}-{}", event.session_id, event.session_index);
        let new_change =
            match NewBalanceChange::from_content(consumer_id, unique_request_id, &content) {
                Ok(change) => change,
                Err(e) => {
                    warn!(
                        error = %e,
                        session_id = %session_id,
                        "Failed to serialize balance change content"
                    );
                    return;
                }
            };

        // 3. Create the balance change record
        let balance_change =
            match db::session_event::create_balance_change_with_tx(&mut tx, &new_change).await {
                Ok(bc) => bc,
                Err(e) => {
                    warn!(
                        error = %e,
                        session_id = %session_id,
                        "Failed to create balance change"
                    );
                    return;
                }
            };

        // Commit the transaction before enqueuing the balance change task
        if let Err(e) = tx.commit().await {
            warn!(
                error = %e,
                session_id = %session_id,
                "Failed to commit session event transaction"
            );
            return;
        }

        // 4. Enqueue a settle_balance_change task to apply the balance change asynchronously
        let entry = BalanceChangeTask {
            balance_change_id: balance_change.id as i64,
        };
        let mut storage: RedisStorage<BalanceChangeTask> = (*balance_change_storage).clone();
        if let Err(e) = storage.push(entry).await {
            warn!(
                error = %e,
                session_id = %session_id,
                balance_change_id = balance_change.id,
                "Failed to enqueue balance change task"
            );
        } else {
            info!(
                session_id = %session_id,
                balance_change_id = balance_change.id,
                "Enqueued balance change task"
            );
        }

        return;
    }

    match tx.commit().await {
        Ok(()) => {
            info!(
                session_id = %session_id,
                "Successfully processed deferred event"
            );
        }
        Err(e) => {
            warn!(
                error = %e,
                session_id = %session_id,
                "Failed to commit session event transaction"
            );
        }
    }
}
