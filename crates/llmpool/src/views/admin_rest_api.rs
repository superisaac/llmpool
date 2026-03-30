use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

use crate::db::{self, DbPool, RedisPool};
use crate::defer::BalanceChangeTask;
use crate::middlewares::admin_auth;
use crate::models::{Account, NewAccount, UpdateAccount};
use crate::models::{BalanceChangeContent, NewBalanceChange};
use crate::redis_utils::cache::{self as redis_cache, ApiKeyInfo};

// --- Server State ---

struct AppState {
    pool: DbPool,
    redis_pool: RedisPool,
    balance_change_storage: RedisStorage<BalanceChangeTask>,
}

// --- Error Response ---

/// Standard error response for REST API
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

fn error_response(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: error.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}

// --- Pagination ---

/// Default page size for paginated responses
const DEFAULT_PAGE_SIZE: i64 = 20;
/// Maximum page size allowed
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Deserialize)]
struct PaginationParams {
    /// Page number (1-based), defaults to 1
    #[serde(default = "default_page")]
    page: i64,
    /// Number of items per page, defaults to 20
    #[serde(default = "default_page_size")]
    page_size: i64,
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    DEFAULT_PAGE_SIZE
}

/// Paginated response wrapper
#[derive(Serialize)]
struct PaginatedResponse<T: Serialize> {
    data: Vec<T>,
    pagination: PaginationInfo,
}

#[derive(Serialize)]
struct PaginationInfo {
    page: i64,
    page_size: i64,
    total: i64,
    total_pages: i64,
}

// --- Endpoint Response DTO ---

/// Response DTO for an endpoint (excludes sensitive api_key field)
#[derive(Serialize)]
struct EndpointResponse {
    id: i32,
    name: String,
    api_base: String,
    has_responses_api: bool,
    tags: Vec<String>,
    proxies: Vec<String>,
    status: String,
    description: String,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::LLMEndpoint> for EndpointResponse {
    fn from(ep: crate::models::LLMEndpoint) -> Self {
        Self {
            id: ep.id,
            name: ep.name,
            api_base: ep.api_base,
            has_responses_api: ep.has_responses_api,
            tags: ep.tags,
            proxies: ep.proxies,
            status: ep.status,
            description: ep.description,
            created_at: ep.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: ep.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- Account Response DTO ---

/// Response DTO for an account
#[derive(Serialize)]
struct AccountResponse {
    id: i32,
    name: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

impl From<Account> for AccountResponse {
    fn from(u: Account) -> Self {
        Self {
            id: u.id,
            name: u.name,
            is_active: u.is_active,
            created_at: u.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: u.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

/// Request body for creating a new account
#[derive(Deserialize)]
struct CreateAccountRequest {
    name: String,
    /// Optional initial credit amount to add to the account's fund after creation.
    /// If greater than zero, a credit balance change will be created and enqueued.
    initial_credit: Option<BigDecimal>,
}

// --- Fund Response DTO ---

/// Response DTO for an account's fund
#[derive(Serialize)]
struct FundResponse {
    id: i32,
    account_id: i32,
    cash: String,
    credit: String,
    debt: String,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::Fund> for FundResponse {
    fn from(f: crate::models::Fund) -> Self {
        Self {
            id: f.id,
            account_id: f.account_id,
            cash: f.cash.to_string(),
            credit: f.credit.to_string(),
            debt: f.debt.to_string(),
            created_at: f.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: f.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- ApiCredential Response DTO ---

/// Response DTO for an API key
#[derive(Serialize)]
struct ApiCredentialResponse {
    id: i32,
    account_id: Option<i32>,
    apikey: String,
    label: String,
    is_active: bool,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::ApiCredential> for ApiCredentialResponse {
    fn from(ak: crate::models::ApiCredential) -> Self {
        Self {
            id: ak.id,
            account_id: ak.account_id,
            apikey: ak.apikey,
            label: ak.label,
            is_active: ak.is_active,
            expires_at: ak
                .expires_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
            created_at: ak.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: ak.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- Handlers ---

/// GET /api/v1/endpoints
///
/// Returns a paginated list of OpenAI endpoints.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_endpoints(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Response {
    // Validate and clamp pagination parameters
    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count
    let total = match db::openai::count_endpoints(&state.pool).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count endpoints");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query endpoints",
            );
        }
    };

    // Get paginated endpoints
    let endpoints = match db::openai::list_endpoints_paginated(&state.pool, offset, page_size).await
    {
        Ok(eps) => eps,
        Err(e) => {
            warn!(error = %e, "Failed to list endpoints");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query endpoints",
            );
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<EndpointResponse> = endpoints.into_iter().map(EndpointResponse::from).collect();

    Json(PaginatedResponse {
        data,
        pagination: PaginationInfo {
            page,
            page_size,
            total,
            total_pages,
        },
    })
    .into_response()
}

// --- Account Handlers ---

/// GET /api/v1/accounts
///
/// Returns a paginated list of accounts.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_accounts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Response {
    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count
    let total = match db::account::count_accounts(&state.pool).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count accounts");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query accounts",
            );
        }
    };

    // Get paginated accounts
    let accounts = match db::account::list_accounts_paginated(&state.pool, offset, page_size).await
    {
        Ok(accounts) => accounts,
        Err(e) => {
            warn!(error = %e, "Failed to list accounts");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query accounts",
            );
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<AccountResponse> = accounts.into_iter().map(AccountResponse::from).collect();

    Json(PaginatedResponse {
        data,
        pagination: PaginationInfo {
            page,
            page_size,
            total,
            total_pages,
        },
    })
    .into_response()
}

/// POST /api/v1/accounts
///
/// Creates a new account.
///
/// Request body (JSON):
/// - `name` (required): The name for the new account
/// - `initial_credit` (optional): Initial credit amount to add to the account's fund.
///   If greater than zero, a credit balance change will be created and applied asynchronously.
async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateAccountRequest>,
) -> Response {
    if payload.name.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Name cannot be empty",
        );
    }

    // Validate initial_credit if provided
    if let Some(ref initial_credit) = payload.initial_credit {
        if *initial_credit < BigDecimal::from(0) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "initial_credit must not be negative",
            );
        }
    }

    let new_account = NewAccount {
        name: payload.name.trim().to_string(),
    };

    let account = match db::account::create_account(&state.pool, &new_account).await {
        Ok(account) => account,
        Err(e) => {
            // Check for unique constraint violation
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("idx_accounts_name") {
                    return error_response(
                        StatusCode::CONFLICT,
                        "duplicate_error",
                        "An account with this name already exists",
                    );
                }
            }
            warn!(error = %e, "Failed to create account");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create account",
            );
        }
    };

    // If initial_credit is provided and greater than zero, create a credit balance change
    if let Some(ref initial_credit) = payload.initial_credit {
        if *initial_credit > BigDecimal::from(0) {
            let content = BalanceChangeContent::Credit {
                amount: initial_credit.clone(),
            };
            let unique_request_id = format!("initial-credit-{}", account.id);
            let new_change = match NewBalanceChange::from_content(
                account.id,
                unique_request_id,
                &content,
            ) {
                Ok(change) => change,
                Err(e) => {
                    warn!(error = %e, account_id = account.id, "Failed to serialize initial credit content");
                    return (StatusCode::CREATED, Json(AccountResponse::from(account)))
                        .into_response();
                }
            };

            let balance_change = match db::session_event::create_balance_change(
                &state.pool,
                &new_change,
            )
            .await
            {
                Ok(bc) => bc,
                Err(e) => {
                    warn!(error = %e, account_id = account.id, "Failed to create initial credit balance change record");
                    return (StatusCode::CREATED, Json(AccountResponse::from(account)))
                        .into_response();
                }
            };

            let task = BalanceChangeTask {
                balance_change_id: balance_change.id as i64,
            };
            let mut storage = state.balance_change_storage.clone();
            if let Err(e) = storage.push(task).await {
                warn!(
                    error = %e,
                    account_id = account.id,
                    balance_change_id = balance_change.id,
                    "Failed to enqueue initial credit balance change task"
                );
            }
        }
    }

    (StatusCode::CREATED, Json(AccountResponse::from(account))).into_response()
}

/// GET /api/v1/accounts/:account_id
///
/// Returns a single account by their ID.
async fn get_account_by_id(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<i32>,
) -> Response {
    match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(account)) => Json(AccountResponse::from(account)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Account with id {} not found", account_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            )
        }
    }
}

/// Request body for updating an account
#[derive(Deserialize)]
struct UpdateAccountRequest {
    name: Option<String>,
    is_active: Option<bool>,
}

/// PUT /api/v1/accounts/:account_id
///
/// Updates an existing account. Only the provided fields will be updated.
///
/// Request body (JSON):
/// - `name` (optional): New name for the account
/// - `is_active` (optional): Whether the account is active
async fn update_account_by_id(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<i32>,
    Json(payload): Json<UpdateAccountRequest>,
) -> Response {
    // Validate name if provided
    if let Some(ref name) = payload.name {
        if name.trim().is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "Name cannot be empty",
            );
        }
    }

    let update = UpdateAccount {
        name: payload.name.map(|u| u.trim().to_string()),
        is_active: payload.is_active,
    };

    match db::account::update_account(&state.pool, account_id, &update).await {
        Ok(account) => Json(AccountResponse::from(account)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Account with id {} not found", account_id),
        ),
        Err(e) => {
            // Check for unique constraint violation
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("idx_accounts_name") {
                    return error_response(
                        StatusCode::CONFLICT,
                        "duplicate_error",
                        "An account with this name already exists",
                    );
                }
            }
            warn!(error = %e, "Failed to update account");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to update account",
            )
        }
    }
}

/// GET /api/v1/accounts_by_name/:name
///
/// Returns a single account by their name.
async fn get_account_by_name(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match db::account::get_account_by_name(&state.pool, &name).await {
        Ok(Some(account)) => Json(AccountResponse::from(account)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Account with name '{}' not found", name),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get account by name");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            )
        }
    }
}

// --- Fund Handlers ---

/// GET /api/v1/accounts/:account_id/fund
///
/// Returns the fund (asset) information for a given account.
/// If the account has no fund record yet, returns a default fund with zero balances.
async fn get_account_fund(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<i32>,
) -> Response {
    // Verify the account exists
    match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    }

    match db::fund::find_account_fund(&state.pool, account_id).await {
        Ok(Some(fund)) => Json(FundResponse::from(fund)).into_response(),
        Ok(None) => {
            // Account exists but has no fund record yet, return default zero balances
            Json(FundResponse {
                id: 0,
                account_id: account_id,
                cash: "0".to_string(),
                credit: "0".to_string(),
                debt: "0".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            })
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to get fund for account");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account fund",
            )
        }
    }
}

// --- AccessKey Handlers ---

/// GET /api/v1/accounts/:account_id/apikeys
///
/// Returns a paginated list of API keys for a given account.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_account_apikeys(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Response {
    // Verify the account exists
    match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    }

    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count of API keys for this account
    let total = match db::api::count_api_credentials_by_account(&state.pool, account_id).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count API keys for account");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query API keys",
            );
        }
    };

    // Get paginated API keys
    let keys = match db::api::list_api_credentials_by_account_paginated(
        &state.pool,
        account_id,
        offset,
        page_size,
    )
    .await
    {
        Ok(keys) => keys,
        Err(e) => {
            warn!(error = %e, "Failed to list API keys for account");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query API keys",
            );
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<ApiCredentialResponse> =
        keys.into_iter().map(ApiCredentialResponse::from).collect();

    Json(PaginatedResponse {
        data,
        pagination: PaginationInfo {
            page,
            page_size,
            total,
            total_pages,
        },
    })
    .into_response()
}

/// Request body for creating a new API key
#[derive(Deserialize)]
struct CreateApiKeyRequest {
    /// Optional label describing the purpose of this API key
    #[serde(default)]
    label: String,
}

/// POST /api/v1/accounts/:account_id/apikeys
///
/// Creates a new API key for the specified account.
///
/// Request body (JSON):
/// - `label` (optional): A label describing the purpose of this API key
async fn create_account_apikey(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<i32>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Response {
    // Verify the account exists
    let account = match db::account::get_account_by_id(&state.pool, account_id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    };

    match db::api::create_api_credential_for_account(&state.pool, account_id, &payload.label).await
    {
        Ok(key) => {
            // Cache the new apikey info in Redis
            let info = ApiKeyInfo {
                id: key.id,
                account_id: key.account_id,
                apikey: key.apikey.clone(),
                label: key.label.clone(),
                is_active: key.is_active,
                account_is_active: account.is_active,
            };
            if let Err(e) = redis_cache::set_apikey_info(&state.redis_pool, &key.apikey, info).await
            {
                warn!(error = %e, apikey = %key.apikey, "Failed to cache new apikey info in Redis");
            }
            (StatusCode::CREATED, Json(ApiCredentialResponse::from(key))).into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to create API key for account");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create API key",
            )
        }
    }
}

// --- ApiKey Handlers (by apikey string) ---

/// GET /api/v1/apikeys/:apikey
///
/// Returns the API credential info for the given apikey string.
async fn get_apikey(State(state): State<Arc<AppState>>, Path(apikey): Path<String>) -> Response {
    match db::api::find_api_credential_by_apikey(&state.pool, &apikey).await {
        Ok(Some(key)) => Json(ApiCredentialResponse::from(key)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("API key '{}' not found", apikey),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get API key by apikey string");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query API key",
            )
        }
    }
}

/// DELETE /api/v1/apikeys/:apikey
///
/// Soft-deletes the API credential by setting is_active = false.
/// Also invalidates the Redis cache entry for this apikey.
async fn delete_apikey(State(state): State<Arc<AppState>>, Path(apikey): Path<String>) -> Response {
    match db::api::deactivate_api_credential(&state.pool, &apikey).await {
        Ok(key) => {
            // Invalidate the Redis cache for this apikey
            if let Err(e) = redis_cache::delete_apikey(&state.redis_pool, &apikey).await {
                warn!(error = %e, apikey = %apikey, "Failed to delete apikey cache from Redis");
            }
            Json(ApiCredentialResponse::from(key)).into_response()
        }
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("API key '{}' not found", apikey),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to deactivate API key");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to delete API key",
            )
        }
    }
}

// --- Endpoint Test DTOs ---

// --- Model List Query Params ---

#[derive(Debug, Deserialize)]
struct ListModelsParams {
    /// Filter by endpoint ID
    endpoint_id: Option<i32>,
    /// Filter by endpoint name
    endpoint_name: Option<String>,
    /// Filter by model name (model_id)
    name: Option<String>,
    /// Page number (1-based), defaults to 1
    #[serde(default = "default_page")]
    page: i64,
    /// Number of items per page, defaults to 20
    #[serde(default = "default_page_size")]
    page_size: i64,
}

// --- Model List Handler ---

/// GET /api/v1/models
///
/// Returns a paginated list of OpenAI models with optional filters.
///
/// Query parameters:
/// - `endpoint_id` (optional): Filter by endpoint ID
/// - `endpoint_name` (optional): Filter by endpoint name
/// - `name` (optional): Filter by model name (model_id)
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_models(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListModelsParams>,
) -> Response {
    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    let filter = db::openai::ListModelsFilter {
        endpoint_id: params.endpoint_id,
        endpoint_name: params.endpoint_name,
        name: params.name,
    };

    // Get total count
    let total = match db::openai::count_models_filtered(&state.pool, &filter).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count models");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query models",
            );
        }
    };

    // Get paginated models
    let models =
        match db::openai::list_models_filtered_paginated(&state.pool, &filter, offset, page_size)
            .await
        {
            Ok(models) => models,
            Err(e) => {
                warn!(error = %e, "Failed to list models");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to query models",
                );
            }
        };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<ModelResponse> = models.into_iter().map(ModelResponse::from).collect();

    Json(PaginatedResponse {
        data,
        pagination: PaginationInfo {
            page,
            page_size,
            total,
            total_pages,
        },
    })
    .into_response()
}

/// Request body for creating a new OpenAI endpoint
#[derive(Deserialize)]
struct CreateEndpointRequest {
    name: String,
    api_key: String,
    api_base: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    proxies: Vec<String>,
}

/// Request body for testing an OpenAI endpoint
#[derive(Deserialize)]
struct TestEndpointRequest {
    api_key: String,
    api_base: String,
}

/// Response DTO for a saved model
#[derive(Serialize)]
struct ModelResponse {
    id: i32,
    endpoint_id: i32,
    model_id: String,
    has_chat_completion: bool,
    has_embedding: bool,
    has_image_generation: bool,
    has_speech: bool,
    input_token_price: String,
    output_token_price: String,
    description: String,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::LLMModel> for ModelResponse {
    fn from(m: crate::models::LLMModel) -> Self {
        Self {
            id: m.id,
            endpoint_id: m.endpoint_id,
            model_id: m.model_id,
            has_chat_completion: m.has_chat_completion,
            has_embedding: m.has_embedding,
            has_image_generation: m.has_image_generation,
            has_speech: m.has_speech,
            input_token_price: m.input_token_price.to_string(),
            output_token_price: m.output_token_price.to_string(),
            description: m.description,
            created_at: m.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: m.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

/// Response DTO for an endpoint with its models
#[derive(Serialize)]
struct EndpointWithModelsResponse {
    endpoint: EndpointResponse,
    models: Vec<ModelResponse>,
}

/// Response DTO for a model's detected features
#[derive(Serialize)]
struct ModelFeaturesResponse {
    model_id: String,
    owned_by: String,
    has_chat_completion: bool,
    has_embedding: bool,
    has_image_generation: bool,
    has_speech: bool,
}

/// Response DTO for endpoint feature detection results
#[derive(Serialize)]
struct TestEndpointResponse {
    has_responses_api: bool,
    models: Vec<ModelFeaturesResponse>,
}

// --- Endpoint Create Handler ---

/// POST /api/v1/endpoints
///
/// Creates a new OpenAI-compatible endpoint by detecting its features and saving
/// the endpoint along with its models to the database.
///
/// Request body (JSON):
/// - `name` (required): A display name for the endpoint
/// - `api_key` (required): The API key for the endpoint
/// - `api_base` (required): The base URL of the endpoint
async fn create_endpoint(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateEndpointRequest>,
) -> Response {
    if payload.name.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "name cannot be empty",
        );
    }
    if payload.api_key.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "api_key cannot be empty",
        );
    }
    if payload.api_base.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "api_base cannot be empty",
        );
    }

    // Detect features from the remote API and save to database
    match crate::openai::features::detect_and_save_features(
        &state.pool,
        payload.name.trim(),
        payload.api_key.trim(),
        payload.api_base.trim(),
    )
    .await
    {
        Ok(()) => {
            // Fetch the saved endpoint and update tags if provided
            match db::openai::get_endpoint_by_api_base(&state.pool, payload.api_base.trim()).await {
                Ok(endpoint) => {
                    // Update tags and proxies if the request included them
                    let endpoint = if !payload.tags.is_empty() || !payload.proxies.is_empty() {
                        let update = crate::models::UpdateLLMEndpoint {
                            name: None,
                            api_base: None,
                            api_key: None,
                            has_responses_api: None,
                            tags: if payload.tags.is_empty() {
                                None
                            } else {
                                Some(payload.tags)
                            },
                            proxies: if payload.proxies.is_empty() {
                                None
                            } else {
                                Some(payload.proxies)
                            },
                            status: None,
                            description: None,
                            updated_at: None,
                        };
                        match db::openai::update_endpoint(&state.pool, endpoint.id, &update).await {
                            Ok(ep) => ep,
                            Err(e) => {
                                warn!(error = %e, "Failed to update endpoint tags");
                                endpoint
                            }
                        }
                    } else {
                        endpoint
                    };
                    // Also fetch the models for this endpoint
                    let models =
                        match db::openai::list_models_by_endpoint(&state.pool, endpoint.id).await {
                            Ok(models) => models,
                            Err(e) => {
                                warn!(error = %e, "Failed to list models for endpoint");
                                return error_response(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "database_error",
                                    "Endpoint created but failed to retrieve models",
                                );
                            }
                        };

                    let model_responses: Vec<ModelResponse> =
                        models.into_iter().map(ModelResponse::from).collect();

                    (
                        StatusCode::CREATED,
                        Json(EndpointWithModelsResponse {
                            endpoint: EndpointResponse::from(endpoint),
                            models: model_responses,
                        }),
                    )
                        .into_response()
                }
                Err(e) => {
                    warn!(error = %e, "Failed to retrieve created endpoint");
                    error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "database_error",
                        "Endpoint created but failed to retrieve it",
                    )
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to detect and save endpoint features");
            error_response(
                StatusCode::BAD_GATEWAY,
                "detection_error",
                &format!("Failed to detect and save endpoint features: {}", e),
            )
        }
    }
}

// --- Endpoint Test Handler ---

/// POST /api/v1/endpoints-tests
///
/// Tests an OpenAI-compatible endpoint by detecting its supported features.
/// This does NOT save anything to the database — it only probes the remote API.
///
/// Request body (JSON):
/// - `api_key` (required): The API key for the endpoint
/// - `api_base` (required): The base URL of the endpoint
async fn test_endpoint(Json(payload): Json<TestEndpointRequest>) -> Response {
    if payload.api_key.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "api_key cannot be empty",
        );
    }
    if payload.api_base.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "api_base cannot be empty",
        );
    }

    match crate::openai::features::detect_features(&payload.api_key, &payload.api_base).await {
        Ok(api_features) => {
            let models: Vec<ModelFeaturesResponse> = api_features
                .model_features
                .into_iter()
                .map(|mf| ModelFeaturesResponse {
                    model_id: mf.model.id,
                    owned_by: mf.model.owned_by,
                    has_chat_completion: mf.has_chat_completion,
                    has_embedding: mf.has_embedding,
                    has_image_generation: mf.has_image_generation,
                    has_speech: mf.has_speech,
                })
                .collect();

            Json(TestEndpointResponse {
                has_responses_api: api_features.has_responses_api,
                models,
            })
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to detect endpoint features");
            error_response(
                StatusCode::BAD_GATEWAY,
                "detection_error",
                &format!("Failed to detect endpoint features: {}", e),
            )
        }
    }
}

// --- Endpoint Get/Update Handlers ---

/// Valid status values for an endpoint
const VALID_ENDPOINT_STATUSES: &[&str] = &["online", "offline", "maintenance"];

/// GET /api/v1/endpoint_by_name/:name
///
/// Returns a single endpoint by its name.
async fn get_endpoint_by_name(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match db::openai::get_endpoint_by_name(&state.pool, &name).await {
        Ok(endpoint) => Json(EndpointResponse::from(endpoint)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with name '{}' not found", name),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get endpoint by name");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query endpoint",
            )
        }
    }
}

/// GET /api/v1/endpoints/:endpoint_id
///
/// Returns a single endpoint by its ID.
async fn get_endpoint_by_id(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i32>,
) -> Response {
    match db::openai::get_endpoint(&state.pool, endpoint_id).await {
        Ok(endpoint) => Json(EndpointResponse::from(endpoint)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with id {} not found", endpoint_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get endpoint by id");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query endpoint",
            )
        }
    }
}

/// Request body for updating an OpenAI endpoint
#[derive(Deserialize)]
struct UpdateEndpointRequest {
    name: Option<String>,
    tags: Option<Vec<String>>,
    proxies: Option<Vec<String>>,
    description: Option<String>,
    status: Option<String>,
}

/// PUT /api/v1/endpoints/:endpoint_id
///
/// Updates an existing endpoint. Only the provided fields will be updated.
///
/// Request body (JSON):
/// - `name` (optional): The display name for the endpoint
/// - `tags` (optional): Tags for the endpoint
/// - `proxies` (optional): Proxy configurations
/// - `description` (optional): Description of the endpoint
/// - `status` (optional): Status of the endpoint (online, offline, maintenance)
async fn update_endpoint_by_id(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i32>,
    Json(payload): Json<UpdateEndpointRequest>,
) -> Response {
    // Validate status if provided
    if let Some(ref status) = payload.status {
        if !VALID_ENDPOINT_STATUSES.contains(&status.as_str()) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                &format!(
                    "Invalid status '{}'. Must be one of: {}",
                    status,
                    VALID_ENDPOINT_STATUSES.join(", ")
                ),
            );
        }
    }

    // Validate name if provided
    if let Some(ref name) = payload.name {
        if name.trim().is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "name cannot be empty",
            );
        }
    }

    let update = crate::models::UpdateLLMEndpoint {
        name: payload.name,
        api_base: None,
        api_key: None,
        has_responses_api: None,
        tags: payload.tags,
        proxies: payload.proxies,
        status: payload.status,
        description: payload.description,
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match db::openai::update_endpoint(&state.pool, endpoint_id, &update).await {
        Ok(endpoint) => Json(EndpointResponse::from(endpoint)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with id {} not found", endpoint_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to update endpoint");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to update endpoint",
            )
        }
    }
}

// --- Model Get/Update Handlers ---

/// GET /api/v1/models/:model_id
///
/// Returns a single model by its ID.
async fn get_model_by_id(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<i32>,
) -> Response {
    match db::openai::get_model(&state.pool, model_id).await {
        Ok(model) => Json(ModelResponse::from(model)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Model with id {} not found", model_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get model by id");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query model",
            )
        }
    }
}

/// Request body for updating an OpenAI model
#[derive(Deserialize)]
struct UpdateModelRequest {
    description: Option<String>,
    input_token_price: Option<BigDecimal>,
    output_token_price: Option<BigDecimal>,
}

/// PUT /api/v1/models/:model_id
///
/// Updates an existing model. Only the provided fields will be updated.
///
/// Request body (JSON):
/// - `description` (optional): Description of the model
/// - `input_token_price` (optional): Price per input token
/// - `output_token_price` (optional): Price per output token
async fn update_model_by_id(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<i32>,
    Json(payload): Json<UpdateModelRequest>,
) -> Response {
    // Validate prices if provided (must not be negative)
    if let Some(ref price) = payload.input_token_price {
        if *price < BigDecimal::from(0) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "input_token_price must not be negative",
            );
        }
    }
    if let Some(ref price) = payload.output_token_price {
        if *price < BigDecimal::from(0) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "validation_error",
                "output_token_price must not be negative",
            );
        }
    }

    let update = crate::models::UpdateLLMModel {
        model_id: None,
        has_image_generation: None,
        has_speech: None,
        has_chat_completion: None,
        has_embedding: None,
        input_token_price: payload.input_token_price,
        output_token_price: payload.output_token_price,
        description: payload.description,
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match db::openai::update_model(&state.pool, model_id, &update).await {
        Ok(model) => Json(ModelResponse::from(model)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Model with id {} not found", model_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to update model");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to update model",
            )
        }
    }
}

// --- Deposit / Withdraw DTOs ---

/// Request body for creating a deposit
#[derive(Deserialize)]
struct CreateDepositRequest {
    account_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a withdrawal
#[derive(Deserialize)]
struct CreateWithdrawRequest {
    account_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a credit
#[derive(Deserialize)]
struct CreateCreditRequest {
    account_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Response DTO for a balance change (deposit)
#[derive(Serialize)]
struct BalanceChangeResponse {
    id: i32,
    account_id: i32,
    unique_request_id: String,
    content: serde_json::Value,
    is_applied: bool,
    created_at: String,
}

impl From<crate::models::BalanceChange> for BalanceChangeResponse {
    fn from(bc: crate::models::BalanceChange) -> Self {
        Self {
            id: bc.id,
            account_id: bc.account_id,
            unique_request_id: bc.unique_request_id,
            content: bc.content,
            is_applied: bc.is_applied,
            created_at: bc.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- Deposit Handler ---

/// POST /api/v1/deposits
///
/// Creates a deposit for an account. This creates a BalanceChange record and enqueues
/// a BalanceChangeTask to apply it asynchronously.
///
/// Request body (JSON):
/// - `account_id` (required): The ID of the account to deposit to
/// - `amount` (required): The deposit amount (must be positive)
///
/// Returns the balance_change_id of the created deposit.
async fn create_deposit(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateDepositRequest>,
) -> Response {
    // Validate amount is positive
    if payload.amount <= BigDecimal::from(0) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Amount must be positive",
        );
    }

    // Verify the account exists
    match db::account::get_account_by_id(&state.pool, payload.account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", payload.account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Deposit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.account_id,
        payload.unique_request_id,
        &content,
    ) {
        Ok(change) => change,
        Err(e) => {
            warn!(error = %e, "Failed to serialize balance change content");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "Failed to serialize deposit content",
            );
        }
    };

    // Create the balance change record in the database
    let balance_change =
        match db::session_event::create_balance_change(&state.pool, &new_change).await {
            Ok(bc) => bc,
            Err(e) => {
                warn!(error = %e, "Failed to create balance change record");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to create deposit record",
                );
            }
        };

    // Enqueue a BalanceChangeTask to apply the balance change asynchronously
    let task = BalanceChangeTask {
        balance_change_id: balance_change.id as i64,
    };
    let mut storage = state.balance_change_storage.clone();
    if let Err(e) = storage.push(task).await {
        warn!(
            error = %e,
            balance_change_id = balance_change.id,
            "Failed to enqueue balance change task"
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "queue_error",
            "Deposit record created but failed to enqueue processing task",
        );
    }

    (
        StatusCode::CREATED,
        Json(BalanceChangeResponse::from(balance_change)),
    )
        .into_response()
}

// --- Withdraw Handler ---

/// POST /api/v1/withdrawals
///
/// Creates a withdrawal for an account. This first checks that the account's fund has
/// sufficient cash (cash >= amount). If not, returns an error. Otherwise, creates
/// a BalanceChange record and enqueues a BalanceChangeTask to apply it asynchronously.
///
/// Request body (JSON):
/// - `account_id` (required): The ID of the account to withdraw from
/// - `amount` (required): The withdrawal amount (must be positive)
///
/// Returns the created BalanceChange record.
async fn create_withdraw(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateWithdrawRequest>,
) -> Response {
    // Validate amount is positive
    if payload.amount <= BigDecimal::from(0) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Amount must be positive",
        );
    }

    // Verify the account exists
    match db::account::get_account_by_id(&state.pool, payload.account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", payload.account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    }

    // Check that the account's fund has sufficient cash
    match db::fund::find_account_fund(&state.pool, payload.account_id).await {
        Ok(Some(fund)) => {
            if fund.cash < payload.amount {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "insufficient_funds",
                    &format!(
                        "Insufficient cash balance. Available: {}, requested: {}",
                        fund.cash, payload.amount
                    ),
                );
            }
        }
        Ok(None) => {
            // Account has no fund record, so cash is effectively 0
            return error_response(
                StatusCode::BAD_REQUEST,
                "insufficient_funds",
                &format!(
                    "Insufficient cash balance. Available: 0, requested: {}",
                    payload.amount
                ),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to query account fund");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account fund",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Withdraw {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.account_id,
        payload.unique_request_id,
        &content,
    ) {
        Ok(change) => change,
        Err(e) => {
            warn!(error = %e, "Failed to serialize balance change content");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "Failed to serialize withdrawal content",
            );
        }
    };

    // Create the balance change record in the database
    let balance_change =
        match db::session_event::create_balance_change(&state.pool, &new_change).await {
            Ok(bc) => bc,
            Err(e) => {
                warn!(error = %e, "Failed to create balance change record");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to create withdrawal record",
                );
            }
        };

    // Enqueue a BalanceChangeTask to apply the balance change asynchronously
    let task = BalanceChangeTask {
        balance_change_id: balance_change.id as i64,
    };
    let mut storage = state.balance_change_storage.clone();
    if let Err(e) = storage.push(task).await {
        warn!(
            error = %e,
            balance_change_id = balance_change.id,
            "Failed to enqueue balance change task"
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "queue_error",
            "Withdrawal record created but failed to enqueue processing task",
        );
    }

    (
        StatusCode::CREATED,
        Json(BalanceChangeResponse::from(balance_change)),
    )
        .into_response()
}

// --- Credit Handler ---

/// POST /api/v1/credits
///
/// Creates a credit for an account. This creates a BalanceChange record and enqueues
/// a BalanceChangeTask to apply it asynchronously. Unlike deposits which add to
/// the cash field, credits add to the credit field of the account's fund.
///
/// Request body (JSON):
/// - `account_id` (required): The ID of the account to credit
/// - `amount` (required): The credit amount (must be positive)
///
/// Returns the created BalanceChange record.
async fn create_credit(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateCreditRequest>,
) -> Response {
    // Validate amount is positive
    if payload.amount <= BigDecimal::from(0) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Amount must be positive",
        );
    }

    // Verify the account exists
    match db::account::get_account_by_id(&state.pool, payload.account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Account with id {} not found", payload.account_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get account by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query account",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Credit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.account_id,
        payload.unique_request_id,
        &content,
    ) {
        Ok(change) => change,
        Err(e) => {
            warn!(error = %e, "Failed to serialize balance change content");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                "Failed to serialize credit content",
            );
        }
    };

    // Create the balance change record in the database
    let balance_change =
        match db::session_event::create_balance_change(&state.pool, &new_change).await {
            Ok(bc) => bc,
            Err(e) => {
                warn!(error = %e, "Failed to create balance change record");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to create credit record",
                );
            }
        };

    // Enqueue a BalanceChangeTask to apply the balance change asynchronously
    let task = BalanceChangeTask {
        balance_change_id: balance_change.id as i64,
    };
    let mut storage = state.balance_change_storage.clone();
    if let Err(e) = storage.push(task).await {
        warn!(
            error = %e,
            balance_change_id = balance_change.id,
            "Failed to enqueue balance change task"
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "queue_error",
            "Credit record created but failed to enqueue processing task",
        );
    }

    (
        StatusCode::CREATED,
        Json(BalanceChangeResponse::from(balance_change)),
    )
        .into_response()
}

// --- Endpoint Tags Handlers ---

/// Response DTO for endpoint tags
#[derive(Serialize)]
struct TagsResponse {
    endpoint_id: i32,
    tags: Vec<String>,
}

/// Request body for adding a tag to an endpoint
#[derive(Deserialize)]
struct AddTagRequest {
    tag: String,
}

/// GET /api/v1/endpoints/:endpoint_id/tags
///
/// Returns the list of tags for a given endpoint.
async fn list_endpoint_tags(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i32>,
) -> Response {
    match db::openai::get_endpoint_tags(&state.pool, endpoint_id).await {
        Ok(tags) => Json(TagsResponse { endpoint_id, tags }).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with id {} not found", endpoint_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get endpoint tags");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query endpoint tags",
            )
        }
    }
}

/// POST /api/v1/endpoints/:endpoint_id/tags
///
/// Adds a tag to the specified endpoint. If the tag already exists, it is not duplicated.
///
/// Request body (JSON):
/// - `tag` (required): The tag string to add
async fn add_endpoint_tag(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i32>,
    Json(payload): Json<AddTagRequest>,
) -> Response {
    if payload.tag.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Tag cannot be empty",
        );
    }

    let tag = payload.tag.trim();

    match db::openai::add_endpoint_tag(&state.pool, endpoint_id, tag).await {
        Ok(endpoint) => (
            StatusCode::OK,
            Json(TagsResponse {
                endpoint_id,
                tags: endpoint.tags,
            }),
        )
            .into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with id {} not found", endpoint_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to add tag to endpoint");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to add tag to endpoint",
            )
        }
    }
}

/// DELETE /api/v1/endpoints/:endpoint_id/tags/:tag
///
/// Removes a tag from the specified endpoint.
async fn remove_endpoint_tag(
    State(state): State<Arc<AppState>>,
    Path((endpoint_id, tag)): Path<(i32, String)>,
) -> Response {
    match db::openai::remove_endpoint_tag(&state.pool, endpoint_id, &tag).await {
        Ok(endpoint) => Json(TagsResponse {
            endpoint_id,
            tags: endpoint.tags,
        })
        .into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Endpoint with id {} not found", endpoint_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to remove tag from endpoint");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to remove tag from endpoint",
            )
        }
    }
}

// --- Session Event Response DTO ---

/// Response DTO for a session event
#[derive(Serialize)]
struct SessionEventResponse {
    id: i64,
    session_id: String,
    session_index: i32,
    account_id: i32,
    model_id: i32,
    api_key_id: i32,
    input_token_price: String,
    input_tokens: i64,
    output_token_price: String,
    output_tokens: i64,
    event_data: serde_json::Value,
    created_at: String,
}

impl From<crate::models::SessionEvent> for SessionEventResponse {
    fn from(e: crate::models::SessionEvent) -> Self {
        Self {
            id: e.id,
            session_id: e.session_id,
            session_index: e.session_index,
            account_id: e.account_id,
            model_id: e.model_id,
            api_key_id: e.api_key_id,
            input_token_price: e.input_token_price.to_string(),
            input_tokens: e.input_tokens,
            output_token_price: e.output_token_price.to_string(),
            output_tokens: e.output_tokens,
            event_data: e.event_data,
            created_at: e.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- Session Event Query Params ---

/// Default count for cursor-based pagination
const DEFAULT_CURSOR_COUNT: i64 = 20;
/// Maximum count for cursor-based pagination
const MAX_CURSOR_COUNT: i64 = 100;

#[derive(Debug, Deserialize)]
struct ListSessionEventsParams {
    /// Filter by session_id
    session: Option<String>,
    /// Cursor: start from this event ID (exclusive), defaults to 0 (from beginning)
    #[serde(default)]
    start: i64,
    /// Number of items to return, defaults to 20, max 100
    #[serde(default = "default_cursor_count")]
    count: i64,
}

fn default_cursor_count() -> i64 {
    DEFAULT_CURSOR_COUNT
}

/// Cursor-based response wrapper for session events
#[derive(Serialize)]
struct CursorResponse<T: Serialize> {
    data: Vec<T>,
    next_id: i64,
    has_more: bool,
}

// --- Session Event Handler ---

/// GET /api/v1/sessionevents
///
/// Returns a cursor-paginated list of session events with optional session_id filter.
///
/// Query parameters:
/// - `session` (optional): Filter by session_id
/// - `start` (optional, default: 0): Cursor event ID to start after (exclusive)
/// - `count` (optional, default: 20, max: 100): Number of items to return
async fn list_session_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListSessionEventsParams>,
) -> Response {
    let start = if params.start < 0 { 0 } else { params.start };
    let count = params.count.clamp(1, MAX_CURSOR_COUNT);

    let session_ref = params.session.as_deref();

    // Fetch count+1 rows to determine has_more
    let mut events =
        match db::session_event::list_session_events_cursor(&state.pool, session_ref, start, count)
            .await
        {
            Ok(events) => events,
            Err(e) => {
                warn!(error = %e, "Failed to list session events");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to query session events",
                );
            }
        };

    let has_more = events.len() as i64 > count;
    if has_more {
        events.truncate(count as usize);
    }

    let next_id = events.last().map(|e| e.id).unwrap_or(start);

    let data: Vec<SessionEventResponse> =
        events.into_iter().map(SessionEventResponse::from).collect();

    Json(CursorResponse {
        data,
        next_id,
        has_more,
    })
    .into_response()
}

// --- Router ---

pub fn get_router(
    pool: DbPool,
    redis_pool: RedisPool,
    balance_change_storage: RedisStorage<BalanceChangeTask>,
) -> Router {
    let state = Arc::new(AppState {
        pool,
        redis_pool,
        balance_change_storage,
    });
    Router::new()
        .route("/endpoints", get(list_endpoints).post(create_endpoint))
        .route(
            "/endpoints/{endpoint_id}",
            get(get_endpoint_by_id).put(update_endpoint_by_id),
        )
        .route("/models", get(list_models))
        .route(
            "/models/{model_id}",
            get(get_model_by_id).put(update_model_by_id),
        )
        .route("/accounts", get(list_accounts).post(create_account))
        .route(
            "/accounts/{account_id}",
            get(get_account_by_id).put(update_account_by_id),
        )
        .route("/accounts/{account_id}/fund", get(get_account_fund))
        .route(
            "/accounts/{account_id}/apikeys",
            get(list_account_apikeys).post(create_account_apikey),
        )
        .route("/accounts_by_name/{name}", get(get_account_by_name))
        .route("/endpoint_by_name/{name}", get(get_endpoint_by_name))
        .route(
            "/endpoints/{endpoint_id}/tags",
            get(list_endpoint_tags).post(add_endpoint_tag),
        )
        .route(
            "/endpoints/{endpoint_id}/tags/{tag}",
            delete(remove_endpoint_tag),
        )
        .route("/endpoint-tests", post(test_endpoint))
        .route("/sessionevents", get(list_session_events))
        .route("/deposits", post(create_deposit))
        .route("/withdrawals", post(create_withdraw))
        .route("/credits", post(create_credit))
        .route("/apikeys/{apikey}", get(get_apikey).delete(delete_apikey))
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))
        .with_state(state)
}
