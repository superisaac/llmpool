use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use apalis_redis::RedisStorage;

use crate::db::{self, DbPool, RedisPool};
use crate::defer::OpenAIEventTask;
use crate::models::{Account, ApiCredential};
use crate::redis_utils::caches::apikey::{self as redis_cache, ApiKeyInfo};

tokio::task_local! {
    pub static ACCOUNT: Account;
    pub static API_CREDENTIAL: ApiCredential;
}

// --- Server State ---

pub struct AppState {
    pub pool: DbPool,
    pub redis_pool: RedisPool,
    pub event_storage: RedisStorage<OpenAIEventTask>,
}

// --- Auth Middleware ---

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

    // Step 1: Try Redis cache first for apikey info
    let cached_info = match redis_cache::get_apikey_info(&state.redis_pool, token).await {
        Ok(info) => info,
        Err(e) => {
            // Cache error is non-fatal; fall through to DB lookup
            warn!(error = %e, "Redis cache error during apikey lookup, falling back to DB");
            None
        }
    };

    if let Some(info) = cached_info {
        // Validate cached info
        if !info.is_active {
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

        // Reconstruct ApiCredential and Account from cached info for task-locals
        let access_key =
            match db::api::find_active_api_credential_by_apikey(&state.pool, token).await {
                Ok(Some(key)) => key,
                Ok(None) => {
                    // Cache may be stale; invalidate and reject
                    let _ = redis_cache::delete_apikey(&state.redis_pool, token).await;
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
        let account = match db::account::get_account_by_id(&state.pool, account_id).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                let _ = redis_cache::delete_apikey(&state.redis_pool, token).await;
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

        return API_CREDENTIAL
            .scope(access_key, ACCOUNT.scope(account, next.run(request)))
            .await;
    }

    // Step 1 (cache miss): Look up the API key from DB
    let access_key = match db::api::find_active_api_credential_by_apikey(&state.pool, token).await {
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

    // Step 4: Populate Redis cache for future requests
    let info = ApiKeyInfo {
        id: access_key.id,
        account_id: access_key.account_id,
        apikey: access_key.apikey.clone(),
        label: access_key.label.clone(),
        is_active: access_key.is_active,
        account_is_active: account.is_active,
    };
    if let Err(e) = redis_cache::set_apikey_info(&state.redis_pool, token, info).await {
        warn!(error = %e, "Failed to cache apikey info in Redis");
    }

    // Step 5: Set task-local variables and proceed
    API_CREDENTIAL
        .scope(access_key, ACCOUNT.scope(account, next.run(request)))
        .await
}

// --- Helpers ---

/// Check if the account has sufficient funds (cash + credit > 0).
/// Tries Redis cache first, falls back to DB on cache miss.
/// Returns Ok(()) if funds are sufficient, Err(Response) with a payment-required error otherwise.
pub async fn check_fund_balance(state: &AppState, account_id: i32) -> Result<(), Response> {
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

    if fund.cash.clone() + fund.credit.clone() <= BigDecimal::from(0) {
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

/// Select the first available endpoint from the database.
/// Returns an error Response if no endpoint is found.
pub(super) async fn select_first_endpoint(
    state: &AppState,
) -> Result<crate::models::LLMEndpoint, Response> {
    match db::openai::list_endpoints(&state.pool).await {
        Ok(endpoints) if !endpoints.is_empty() => Ok(endpoints.into_iter().next().unwrap()),
        Ok(_) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": {
                    "message": "No upstream endpoints configured.",
                    "type": "server_error",
                    "code": "no_endpoint"
                }
            })),
        )
            .into_response()),
        Err(e) => {
            warn!(error = %e, "Failed to query endpoints for files/batches proxy");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "Internal server error while selecting endpoint.",
                        "type": "server_error",
                        "code": "internal_error"
                    }
                })),
            )
                .into_response())
        }
    }
}

/// Build an async-openai Client from an LLMEndpoint (no model_id needed).
pub(super) fn build_client_from_endpoint(
    endpoint: &crate::models::LLMEndpoint,
) -> async_openai::Client<async_openai::config::OpenAIConfig> {
    use async_openai::{Client, config::OpenAIConfig};

    let config = OpenAIConfig::new()
        .with_api_key(endpoint.api_key.clone())
        .with_api_base(endpoint.api_base.clone());

    if !endpoint.proxies.is_empty() {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if let Some(proxy_url) = endpoint.proxies.choose(&mut rng) {
            info!(
                endpoint_name = %endpoint.name,
                proxy = %proxy_url,
                "OpenAI proxy: using proxy for endpoint (batches)"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            let http_client = reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy");
            return Client::with_config(config).with_http_client(http_client);
        }
    }
    Client::with_config(config)
}

/// Build a Client from an (LLMModel, LLMEndpoint) pair.
/// If the endpoint has proxies configured, a random one is selected and used.
pub(super) fn build_client_from_model_endpoint(
    model: &crate::models::LLMModel,
    endpoint: &crate::models::LLMEndpoint,
) -> (
    async_openai::Client<async_openai::config::OpenAIConfig>,
    i32,
) {
    use async_openai::{Client, config::OpenAIConfig};

    let config = OpenAIConfig::new()
        .with_api_key(endpoint.api_key.clone())
        .with_api_base(endpoint.api_base.clone());

    let client = if !endpoint.proxies.is_empty() {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if let Some(proxy_url) = endpoint.proxies.choose(&mut rng) {
            info!(
                endpoint_name = %endpoint.name,
                proxy = %proxy_url,
                "OpenAI proxy: using proxy for endpoint"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            let http_client = reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .expect("Failed to build reqwest client with proxy");
            Client::with_config(config).with_http_client(http_client)
        } else {
            Client::with_config(config)
        }
    } else {
        Client::with_config(config)
    };

    (client, model.id)
}

/// Returns up to `count` (Client, model_db_id) pairs selected by lowest current-hour output
/// token usage from Redis. If a model has no Redis key, its usage defaults to 0.
pub(super) async fn select_model_clients(
    db_pool: &DbPool,
    redis_pool: &RedisPool,
    model_name: &str,
    capacity: &crate::models::CapacityOption,
    count: usize,
) -> Vec<(
    async_openai::Client<async_openai::config::OpenAIConfig>,
    i32,
)> {
    use crate::redis_utils::counters::get_output_token_usage_batch;

    let models =
        match db::openai::find_models_by_name_and_capacity(db_pool, model_name, capacity).await {
            Ok(models) if !models.is_empty() => models,
            Ok(_) => {
                warn!(
                    model = model_name,
                    "No models found in DB for the requested capacity"
                );
                return vec![];
            }
            Err(e) => {
                warn!(
                    model = model_name,
                    error = %e,
                    "DB query failed when looking up models"
                );
                return vec![];
            }
        };

    // Fetch output token usage from Redis for all models in a single MGET, then sort ascending
    // and take the `count` with the lowest usage.
    let model_ids: Vec<i32> = models.iter().map(|(m, _)| m.id).collect();
    let usages = get_output_token_usage_batch(redis_pool, &model_ids).await;

    let mut models_with_usage: Vec<(i64, &(crate::models::LLMModel, crate::models::LLMEndpoint))> =
        usages.into_iter().zip(models.iter()).collect();

    models_with_usage.sort_by_key(|(usage, _)| *usage);
    models_with_usage.truncate(count);

    models_with_usage
        .into_iter()
        .map(|(usage, (model, endpoint))| {
            info!(
                model = model_name,
                endpoint_name = endpoint.name,
                api_base = endpoint.api_base,
                output_token_usage = usage,
                "Selected endpoint candidate by lowest output token usage"
            );
            build_client_from_model_endpoint(model, endpoint)
        })
        .collect()
}
