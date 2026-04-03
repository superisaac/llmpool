use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::warn;

use crate::db;
use crate::models::{Account, ApiCredential};
use crate::redis_utils::caches::api_key::{self as redis_cache, ApiKeyInfo};
use crate::views::openai_proxy::helpers::AppState;

/// Compute the SHA-256 hex hash of a plaintext API key token.
/// Used as the Redis cache key and for cache-based comparison.
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

tokio::task_local! {
    pub static ACCOUNT: Account;
    pub static API_CREDENTIAL: ApiCredential;
}

/// Helper to build a JSON error response for authentication failures.
fn auth_error_response(status: StatusCode, message: &str, code: &str) -> Response {
    let error_type = if status == StatusCode::UNAUTHORIZED {
        "authentication_error"
    } else {
        "server_error"
    };
    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": code
            }
        })),
    )
        .into_response()
}

/// Middleware that authenticates requests using Bearer token from the Authorization header.
/// It looks up the ACCESS_KEY by apikey (checking Redis cache first, then DB on miss),
/// checks that it is active, then finds the account and checks that it is active.
/// Both ACCESS_KEY and ACCOUNT are stored in task-local variables for downstream handlers.
pub async fn auth_openai_api(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    // Extract the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Missing or invalid Authorization header. Expected: Bearer <apikey>",
                "invalid_api_key",
            );
        }
    };

    // Compute the SHA-256 hash of the incoming token — used as the Redis cache key.
    let token_hash = hash_token(token);

    // Step 1: Try Redis cache first for apikey info (keyed by hash)
    let cached_info = match redis_cache::get_apikey_info(&state.redis_pool, &token_hash).await {
        Ok(info) => info,
        Err(e) => {
            // Cache error is non-fatal; fall through to DB lookup
            warn!(error = %e, "Redis cache error during apikey lookup, falling back to DB");
            None
        }
    };

    if let Some(info) = cached_info {
        // Validate cached info — compare hash to guard against cache key collisions
        if info.api_key_hash != token_hash || !info.is_active {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Invalid API key.",
                "invalid_api_credential",
            );
        }
        let account_id = match info.account_id {
            Some(id) => id,
            None => {
                return auth_error_response(
                    StatusCode::UNAUTHORIZED,
                    "API key is not associated with an account.",
                    "invalid_api_credential",
                );
            }
        };
        if !info.account_is_active {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Account is inactive.",
                "invalid_api_credential",
            );
        }

        // Reconstruct ApiCredential and Account directly from cached info — no DB queries needed.
        // Downstream handlers only access `.id` on both structs, so placeholder values are safe
        // for the remaining fields.
        let now = chrono::Utc::now().naive_utc();
        let access_key = crate::models::ApiCredential {
            id: info.id,
            account_id: info.account_id,
            encrypted_api_key: String::new(),
            ellipsed_api_key: String::new(),
            api_key_hash: info.api_key_hash,
            apikey: String::new(),
            label: info.label,
            is_active: info.is_active,
            expires_at: None,
            created_at: now,
            updated_at: now,
        };
        let account = crate::models::Account {
            id: account_id,
            name: String::new(),
            is_active: info.account_is_active,
            created_at: now,
            updated_at: now,
        };

        return API_CREDENTIAL
            .scope(access_key, ACCOUNT.scope(account, next.run(request)))
            .await;
    }

    // Step 1 (cache miss): Look up the API key from DB by hash (already computed above)
    let access_key =
        match db::api::find_active_api_credential_by_api_key_hash(&state.pool, &token_hash).await {
            Ok(Some(key)) => key,
            Ok(None) => {
                return auth_error_response(
                    StatusCode::UNAUTHORIZED,
                    "Invalid API key.",
                    "invalid_api_credential",
                );
            }
            Err(e) => {
                warn!(error = %e, "Database error during API key lookup");
                return auth_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error during authentication.",
                    "internal_error",
                );
            }
        };

    // Step 2: Find the account by ACCESS_KEY.account_id (if present)
    let account_id = match access_key.account_id {
        Some(uid) => uid,
        None => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "API key is not associated with an account.",
                "invalid_api_credential",
            );
        }
    };

    let account = match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                "Account not found for this API key.",
                "invalid_api_credential",
            );
        }
        Err(e) => {
            warn!(error = %e, "Database error during account lookup");
            return auth_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error during authentication.",
                "internal_error",
            );
        }
    };

    // Step 3: Check if the account is active
    if !account.is_active {
        return auth_error_response(
            StatusCode::UNAUTHORIZED,
            "Account is inactive.",
            "invalid_api_credential",
        );
    }

    // Step 4: Populate Redis cache for future requests (keyed by hash)
    let info = ApiKeyInfo {
        id: access_key.id,
        account_id: access_key.account_id,
        api_key_hash: token_hash.clone(),
        label: access_key.label.clone(),
        is_active: access_key.is_active,
        account_is_active: account.is_active,
    };
    if let Err(e) = redis_cache::set_apikey_info(&state.redis_pool, &token_hash, info).await {
        warn!(error = %e, "Failed to cache apikey info in Redis");
    }

    // Step 5: Set task-local variables and proceed
    API_CREDENTIAL
        .scope(access_key, ACCOUNT.scope(account, next.run(request)))
        .await
}
