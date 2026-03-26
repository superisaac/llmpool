use apalis::prelude::*;
use apalis_redis::RedisStorage;
use bigdecimal::BigDecimal;
use tracing::{info, warn};

use crate::db::{self, DbPool};
use crate::defer::{BalanceChangeTask, OpenAIEventData, OpenAIEventTask};
use crate::models::{BalanceChangeContent, NewBalanceChange, NewSessionEvent, SpendToken};

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
    let user_id = event.user_id;
    let model_id = event.model_id;

    info!(
        session_id = %session_id,
        session_index = event.session_index,
        user_id = user_id,
        model_id = model_id,
        "Processing deferred event"
    );

    // Extract usage before serializing
    let usage = extract_usage(&event.event_data);

    let event_data_json =
        serde_json::to_value(&event.event_data).unwrap_or(serde_json::Value::Null);
    let new_event = NewSessionEvent {
        session_id: session_id.clone(),
        session_index: event.session_index,
        user_id,
        model_id,
        event_data: event_data_json,
    };

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

    // 2. If there's usage info, look up model token prices and create a balance change
    if let Some(usage) = usage {
        // Look up the model to get token prices
        let model = match db::openai::get_model_with_tx(&mut tx, model_id).await {
            Ok(model) => model,
            Err(e) => {
                warn!(
                    error = %e,
                    session_id = %session_id,
                    "Failed to look up model"
                );
                return;
            }
        };

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
        let new_change = match NewBalanceChange::from_content(user_id, &content) {
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
