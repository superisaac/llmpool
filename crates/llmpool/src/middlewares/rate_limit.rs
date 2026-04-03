use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use bb8_redis::redis::AsyncCommands;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::warn;

use crate::config;
use crate::db::RedisPool;

/// State passed to the rate limiting middleware.
#[derive(Clone)]
pub struct RateLimitState {
    pub redis_pool: RedisPool,
}

/// Build a JSON 429 Too Many Requests response.
fn rate_limit_response(message: &str) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "rate_limit_error",
                "code": "rate_limit_exceeded"
            }
        })),
    )
        .into_response()
}

/// Sliding-window rate limit check using Redis.
///
/// Uses a Redis sorted set keyed by `ratelimit:<kind>:<identifier>`.
/// Each request adds the current timestamp (ms) as score with a unique member,
/// removes entries older than the window, then checks the count.
///
/// Returns `Ok(())` if the request is allowed, `Err(Response)` if rate limited.
async fn check_rate_limit(
    redis_pool: &RedisPool,
    kind: &str,
    identifier: &str,
    max_requests: u64,
    window_secs: u64,
) -> Result<(), Response> {
    let mut conn = match redis_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            warn!(
                error = %e,
                "Failed to get Redis connection for rate limiting; allowing request"
            );
            return Ok(());
        }
    };

    let key = format!("ratelimit:{}:{}", kind, identifier);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let window_ms = (window_secs * 1000) as i64;
    let window_start = now_ms - window_ms;

    // Step 1: Remove entries older than the sliding window
    if let Err(e) = conn
        .zrembyscore::<_, _, _, ()>(&key, "-inf", window_start)
        .await
    {
        warn!(
            error = %e,
            key = %key,
            "Redis ZREMRANGEBYSCORE failed; allowing request"
        );
        return Ok(());
    }

    // Step 2: Add the current request with a unique member (timestamp + random u64)
    let rand_suffix: u64 = rand::rng().random();
    let member = format!("{}-{}", now_ms, rand_suffix);
    if let Err(e) = conn.zadd::<_, _, _, ()>(&key, member.as_str(), now_ms).await {
        warn!(
            error = %e,
            key = %key,
            "Redis ZADD failed; allowing request"
        );
        return Ok(());
    }

    // Step 3: Count entries in the current window
    let count: u64 = match conn.zcard(&key).await {
        Ok(c) => c,
        Err(e) => {
            warn!(
                error = %e,
                key = %key,
                "Redis ZCARD failed; allowing request"
            );
            return Ok(());
        }
    };

    // Step 4: Refresh TTL so the key expires after one window of inactivity
    if let Err(e) = conn.expire::<_, ()>(&key, window_secs as i64).await {
        warn!(error = %e, key = %key, "Redis EXPIRE failed");
    }

    if count > max_requests {
        return Err(rate_limit_response(&format!(
            "Rate limit exceeded: more than {} requests per {} seconds ({})",
            max_requests, window_secs, kind
        )));
    }

    Ok(())
}

/// Axum middleware that enforces rate limits on the `/openai/v1` routes.
///
/// Two independent limits are applied per request:
/// 1. **IP-based**: keyed on the client's remote IP address.
/// 2. **Token-based**: keyed on a SHA-256 hash of the `Authorization` header value.
///
/// Both limits use a Redis sliding-window counter.
/// If rate limiting is disabled in config (`[rate_limit] enabled = false`), this is a no-op.
pub async fn rate_limit_middleware(
    State(state): State<Arc<RateLimitState>>,
    request: Request,
    next: Next,
) -> Response {
    let cfg = config::get_config();
    let rl_cfg = &cfg.rate_limit;

    // Short-circuit if rate limiting is disabled
    if !rl_cfg.enabled {
        return next.run(request).await;
    }

    // --- 1. IP-based rate limiting ---
    let client_ip = extract_client_ip(&request);
    if let Some(ref ip) = client_ip {
        if let Err(resp) = check_rate_limit(
            &state.redis_pool,
            "ip",
            ip,
            rl_cfg.ip_requests,
            rl_cfg.ip_window_secs,
        )
        .await
        {
            return resp;
        }
    }

    // --- 2. Authorization token-based rate limiting ---
    let auth_token = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(ref token) = auth_token {
        // Hash the token so raw API keys are never stored in Redis key names
        let token_hash = sha256_hex(token);
        if let Err(resp) = check_rate_limit(
            &state.redis_pool,
            "token",
            &token_hash,
            rl_cfg.token_requests,
            rl_cfg.token_window_secs,
        )
        .await
        {
            return resp;
        }
    }

    next.run(request).await
}

/// Extract the client IP address from the request.
///
/// Priority:
/// 1. `X-Forwarded-For` header (first IP — the original client)
/// 2. `X-Real-IP` header
/// 3. TCP socket address via `ConnectInfo<SocketAddr>` extension
fn extract_client_ip(request: &Request) -> Option<String> {
    // X-Forwarded-For: client, proxy1, proxy2
    if let Some(xff) = request
        .headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let ip = first.trim().to_string();
            if !ip.is_empty() {
                return Some(ip);
            }
        }
    }

    // X-Real-IP
    if let Some(xri) = request
        .headers()
        .get("X-Real-IP")
        .and_then(|v| v.to_str().ok())
    {
        let ip = xri.trim().to_string();
        if !ip.is_empty() {
            return Some(ip);
        }
    }

    // ConnectInfo socket address (requires axum::serve with into_make_service_with_connect_info)
    if let Some(addr) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Some(addr.0.ip().to_string());
    }

    None
}

/// Compute a SHA-256 hex digest of the input string.
/// Used to avoid storing raw API keys in Redis key names.
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}
