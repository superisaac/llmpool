use bb8_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::db::RedisPool;

/// TTL for apikey cache entries (in seconds): 5 minutes
const APIKEY_CACHE_TTL: u64 = 300;

/// Cached information about an API key and its associated account.
/// This is stored in Redis to avoid repeated database lookups on every request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: i32,
    pub account_id: Option<i32>,
    pub apikey: String,
    pub label: String,
    pub is_active: bool,
    pub account_is_active: bool,
}

type CacheError = Box<dyn std::error::Error + Send + Sync>;

fn apikey_cache_key(apikey: &str) -> String {
    format!("apikey:info:{}", apikey)
}

/// Get cached ApiKeyInfo from Redis for the given apikey string.
/// Returns Ok(Some(info)) if found, Ok(None) if not cached, Err on Redis error.
pub async fn get_apikey_info(
    redis_pool: &RedisPool,
    apikey: &str,
) -> Result<Option<ApiKeyInfo>, CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for apikey cache get");
        Box::new(e) as CacheError
    })?;

    let key = apikey_cache_key(apikey);
    let value: Option<String> = conn.get::<_, Option<String>>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to get apikey info from Redis cache");
        Box::new(e) as CacheError
    })?;

    match value {
        Some(json) => {
            let info: ApiKeyInfo = serde_json::from_str(&json).map_err(|e| {
                warn!(error = %e, key = %key, "Failed to deserialize apikey info from Redis cache");
                Box::new(e) as CacheError
            })?;
            Ok(Some(info))
        }
        None => Ok(None),
    }
}

/// Store ApiKeyInfo in Redis cache for the given apikey string.
/// The entry will expire after APIKEY_CACHE_TTL seconds.
pub async fn set_apikey_info(
    redis_pool: &RedisPool,
    apikey: &str,
    info: ApiKeyInfo,
) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for apikey cache set");
        Box::new(e) as CacheError
    })?;

    let key = apikey_cache_key(apikey);
    let json = serde_json::to_string(&info).map_err(|e| {
        warn!(error = %e, "Failed to serialize apikey info for Redis cache");
        Box::new(e) as CacheError
    })?;

    conn.set_ex::<_, _, ()>(&key, json, APIKEY_CACHE_TTL)
        .await
        .map_err(|e| {
            warn!(error = %e, key = %key, "Failed to set apikey info in Redis cache");
            Box::new(e) as CacheError
        })?;

    Ok(())
}

/// Delete the cached ApiKeyInfo from Redis for the given apikey string.
/// Used when an apikey is deactivated so the cache is immediately invalidated.
pub async fn delete_apikey(redis_pool: &RedisPool, apikey: &str) -> Result<(), CacheError> {
    let mut conn = redis_pool.get().await.map_err(|e| {
        warn!(error = %e, "Failed to get Redis connection for apikey cache delete");
        Box::new(e) as CacheError
    })?;

    let key = apikey_cache_key(apikey);
    conn.del::<_, ()>(&key).await.map_err(|e| {
        warn!(error = %e, key = %key, "Failed to delete apikey info from Redis cache");
        Box::new(e) as CacheError
    })?;

    Ok(())
}
