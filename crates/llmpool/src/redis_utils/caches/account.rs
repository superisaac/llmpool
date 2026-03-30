use bb8_redis::redis::AsyncCommands;
use tracing::warn;

use crate::db::RedisPool;
use crate::models::Account;

/// TTL for account cache entries (in seconds): 5 minutes
const ACCOUNT_CACHE_TTL: u64 = 300;

type CacheError = Box<dyn std::error::Error + Send + Sync>;

fn account_cache_key(account_id: &str) -> String {
    format!("account:info:{}", account_id)
}

/// Get cached Account info from Redis for the given account_id string.
/// Returns Ok(Some(account)) if found, Ok(None) if not cached, Err on Redis error.
pub async fn get_account_info(
    redis_pool: &RedisPool,
    account_id: &str,
) -> Result<Option<Account>, CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for account cache get");
        Box::new(e) as CacheError
    })?;

    let key = account_cache_key(account_id);
    let value: Option<String> = conn.get::<_, Option<String>>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to get account info from Redis cache");
        Box::new(e) as CacheError
    })?;

    match value {
        Some(json) => {
            let account: Account = serde_json::from_str(&json).map_err(|e| {
                warn!(error = %e, key = %key, "Failed to deserialize account info from Redis cache");
                Box::new(e) as CacheError
            })?;
            Ok(Some(account))
        }
        None => Ok(None),
    }
}

/// Store Account info in Redis cache for the given account_id string.
/// The entry will expire after ACCOUNT_CACHE_TTL seconds.
pub async fn set_account_info(
    redis_pool: &RedisPool,
    account_id: &str,
    account: &Account,
) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for account cache set");
        Box::new(e) as CacheError
    })?;

    let key = account_cache_key(account_id);
    let json = serde_json::to_string(account).map_err(|e| {
        warn!(error = %e, "Failed to serialize account info for Redis cache");
        Box::new(e) as CacheError
    })?;

    conn.set_ex::<_, _, ()>(&key, json, ACCOUNT_CACHE_TTL)
        .await
        .map_err(|e| {
            warn!(error = %e, key = %key, "Failed to set account info in Redis cache");
            Box::new(e) as CacheError
        })?;

    Ok(())
}

/// Delete the cached Account info from Redis for the given account_id string.
/// Used when an account is updated so the cache is immediately invalidated.
pub async fn delete_account(redis_pool: &RedisPool, account_id: &str) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for account cache delete");
        Box::new(e) as CacheError
    })?;

    let key = account_cache_key(account_id);
    conn.del::<_, ()>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to delete account info from Redis cache");
        Box::new(e) as CacheError
    })?;

    Ok(())
}
