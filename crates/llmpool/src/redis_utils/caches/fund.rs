use bb8_redis::redis::AsyncCommands;
use tracing::warn;

use crate::db::RedisPool;
use crate::models::Fund;

/// TTL for fund cache entries (in seconds): 5 minutes
const FUND_CACHE_TTL: u64 = 300;

type CacheError = Box<dyn std::error::Error + Send + Sync>;

fn fund_cache_key(account_id: i32) -> String {
    format!("fund:info:{}", account_id)
}

/// Get cached Fund info from Redis for the given fund_id string.
/// Returns Ok(Some(fund)) if found, Ok(None) if not cached, Err on Redis error.
pub async fn get_fund_info(
    redis_pool: &RedisPool,
    account_id: i32,
) -> Result<Option<Fund>, CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for fund cache get");
        Box::new(e) as CacheError
    })?;

    let key = fund_cache_key(account_id);
    let value: Option<String> = conn.get::<_, Option<String>>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to get fund info from Redis cache");
        Box::new(e) as CacheError
    })?;

    match value {
        Some(json) => {
            let fund: Fund = serde_json::from_str(&json).map_err(|e| {
                warn!(error = %e, key = %key, "Failed to deserialize fund info from Redis cache");
                Box::new(e) as CacheError
            })?;
            Ok(Some(fund))
        }
        None => Ok(None),
    }
}

/// Store Fund info in Redis cache for the given fund_id string.
/// The entry will expire after FUND_CACHE_TTL seconds.
pub async fn set_fund_info(
    redis_pool: &RedisPool,
    account_id: i32,
    info: Fund,
) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for fund cache set");
        Box::new(e) as CacheError
    })?;

    let key = fund_cache_key(account_id);
    let json = serde_json::to_string(&info).map_err(|e| {
        warn!(error = %e, "Failed to serialize fund info for Redis cache");
        Box::new(e) as CacheError
    })?;

    conn.set_ex::<_, _, ()>(&key, json, FUND_CACHE_TTL)
        .await
        .map_err(|e| {
            warn!(error = %e, key = %key, "Failed to set fund info in Redis cache");
            Box::new(e) as CacheError
        })?;

    Ok(())
}
