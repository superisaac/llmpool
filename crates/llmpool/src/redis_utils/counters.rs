use bb8_redis::redis::AsyncCommands;
use chrono::Utc;
use tracing::warn;

use crate::db::RedisPool;

/// Get the current hour's output token usage from Redis for a batch of model IDs.
///
/// Uses a single `MGET` command to fetch all values at once.
/// Returns a `Vec<i64>` in the same order as `model_ids`.
/// Missing keys or errors default to `0`.
///
/// Key pattern: `tokenusage:output:<model_id>:<hour>` where `<hour>` is `YYYYMMDDHH` (UTC).
pub async fn get_output_token_usage_batch(redis_pool: &RedisPool, model_ids: &[i32]) -> Vec<i64> {
    if model_ids.is_empty() {
        return vec![];
    }

    let mut redis_conn = match redis_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to get Redis connection from pool for token usage query");
            return vec![0; model_ids.len()];
        }
    };

    let hour = Utc::now().format("%Y%m%d%H").to_string();
    let keys: Vec<String> = model_ids
        .iter()
        .map(|id| format!("tokenusage:output:{}:{}", id, hour))
        .collect();

    match redis_conn.get::<_, Vec<Option<i64>>>(keys.as_slice()).await {
        Ok(values) => values.into_iter().map(|v| v.unwrap_or(0)).collect(),
        Err(e) => {
            warn!(error = %e, "Failed to mget output token usage from Redis");
            vec![0; model_ids.len()]
        }
    }
}

/// Increment the hourly token usage counters in Redis for the given model.
///
/// Keys follow the pattern:
///   `tokenusage:input:<model_id>:<hour>`
///   `tokenusage:output:<model_id>:<hour>`
///
/// where `<hour>` is formatted as `YYYYMMDDHH` (UTC).
pub async fn increment_token_usage(
    redis_pool: &RedisPool,
    model_id: i32,
    input_tokens: i64,
    output_tokens: i64,
) {
    let mut redis_conn = match redis_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Failed to get Redis connection from pool for token usage counter");
            return;
        }
    };

    // Format the current UTC hour as YYYYMMDDHH
    let hour = Utc::now().format("%Y%m%d%H").to_string();
    let input_key = format!("tokenusage:input:{}:{}", model_id, hour);
    let output_key = format!("tokenusage:output:{}:{}", model_id, hour);

    if input_tokens > 0 {
        if let Err(e) = redis_conn
            .incr::<_, i64, i64>(&input_key, input_tokens)
            .await
        {
            warn!(error = %e, key = %input_key, "Failed to increment input token usage in Redis");
        } else if let Err(e) = redis_conn.expire::<_, bool>(&input_key, 3600).await {
            warn!(error = %e, key = %input_key, "Failed to set TTL on input token usage key in Redis");
        }
    }
    if output_tokens > 0 {
        if let Err(e) = redis_conn
            .incr::<_, i64, i64>(&output_key, output_tokens)
            .await
        {
            warn!(error = %e, key = %output_key, "Failed to increment output token usage in Redis");
        } else if let Err(e) = redis_conn.expire::<_, bool>(&output_key, 3600).await {
            warn!(error = %e, key = %output_key, "Failed to set TTL on output token usage key in Redis");
        }
    }
}
