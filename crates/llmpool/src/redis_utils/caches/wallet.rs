use bb8_redis::redis::AsyncCommands;
use tracing::warn;

use crate::db::RedisPool;
use crate::models::Wallet;

/// TTL for wallet cache entries (in seconds): 5 minutes
const FUND_CACHE_TTL: u64 = 300;

type CacheError = Box<dyn std::error::Error + Send + Sync>;

fn wallet_cache_key(account_id: i64) -> String {
    format!("wallet:info:{}", account_id)
}

/// Get cached Wallet info from Redis for the given wallet_id string.
/// Returns Ok(Some(wallet)) if found, Ok(None) if not cached, Err on Redis error.
pub async fn get_wallet_info(
    redis_pool: &RedisPool,
    account_id: i64,
) -> Result<Option<Wallet>, CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for wallet cache get");
        Box::new(e) as CacheError
    })?;

    let key = wallet_cache_key(account_id);
    let value: Option<String> = conn.get::<_, Option<String>>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to get wallet info from Redis cache");
        Box::new(e) as CacheError
    })?;

    match value {
        Some(json) => {
            let wallet: Wallet = serde_json::from_str(&json).map_err(|e| {
                warn!(error = %e, key = %key, "Failed to deserialize wallet info from Redis cache");
                Box::new(e) as CacheError
            })?;
            Ok(Some(wallet))
        }
        None => Ok(None),
    }
}

/// Store Wallet info in Redis cache for the given wallet_id string.
/// The entry will expire after FUND_CACHE_TTL seconds.
pub async fn set_wallet_info(
    redis_pool: &RedisPool,
    account_id: i64,
    info: Wallet,
) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for wallet cache set");
        Box::new(e) as CacheError
    })?;

    let key = wallet_cache_key(account_id);
    let json = serde_json::to_string(&info).map_err(|e| {
        warn!(error = %e, "Failed to serialize wallet info for Redis cache");
        Box::new(e) as CacheError
    })?;

    conn.set_ex::<_, _, ()>(&key, json, FUND_CACHE_TTL)
        .await
        .map_err(|e| {
            warn!(error = %e, key = %key, "Failed to set wallet info in Redis cache");
            Box::new(e) as CacheError
        })?;

    Ok(())
}
