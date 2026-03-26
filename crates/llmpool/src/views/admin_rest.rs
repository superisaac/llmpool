use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

use crate::db::{self, DbPool};
use crate::defer::BalanceChangeTask;
use crate::middlewares::admin_auth;
use crate::models::{BalanceChangeContent, NewBalanceChange};

// --- Server State ---

struct AppState {
    pool: DbPool,
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

impl From<crate::models::OpenAIEndpoint> for EndpointResponse {
    fn from(ep: crate::models::OpenAIEndpoint) -> Self {
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

// --- User Response DTO ---

/// Response DTO for a user
#[derive(Serialize)]
struct UserResponse {
    id: i32,
    username: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::User> for UserResponse {
    fn from(u: crate::models::User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            is_active: u.is_active,
            created_at: u.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: u.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

/// Request body for creating a new user
#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    /// Optional initial credit amount to add to the user's fund after creation.
    /// If greater than zero, a credit balance change will be created and enqueued.
    initial_credit: Option<BigDecimal>,
}

// --- Fund Response DTO ---

/// Response DTO for a user's fund
#[derive(Serialize)]
struct FundResponse {
    id: i32,
    user_id: i32,
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
            user_id: f.user_id,
            cash: f.cash.to_string(),
            credit: f.credit.to_string(),
            debt: f.debt.to_string(),
            created_at: f.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: f.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- AccessKey Response DTO ---

/// Response DTO for an access key
#[derive(Serialize)]
struct AccessKeyResponse {
    id: i32,
    user_id: Option<i32>,
    apikey: String,
    is_active: bool,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::AccessKey> for AccessKeyResponse {
    fn from(ak: crate::models::AccessKey) -> Self {
        Self {
            id: ak.id,
            user_id: ak.user_id,
            apikey: ak.apikey,
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

// --- User Handlers ---

/// GET /api/v1/users
///
/// Returns a paginated list of users.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_users(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Response {
    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count
    let total = match db::user::count_users(&state.pool).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count users");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query users",
            );
        }
    };

    // Get paginated users
    let users = match db::user::list_users_paginated(&state.pool, offset, page_size).await {
        Ok(users) => users,
        Err(e) => {
            warn!(error = %e, "Failed to list users");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query users",
            );
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<UserResponse> = users.into_iter().map(UserResponse::from).collect();

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

/// POST /api/v1/users
///
/// Creates a new user.
///
/// Request body (JSON):
/// - `username` (required): The username for the new user
/// - `initial_credit` (optional): Initial credit amount to add to the user's fund.
///   If greater than zero, a credit balance change will be created and applied asynchronously.
async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateUserRequest>,
) -> Response {
    if payload.username.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "Username cannot be empty",
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

    let new_user = crate::models::NewUser {
        username: payload.username.trim().to_string(),
    };

    let user = match db::user::create_user(&state.pool, &new_user).await {
        Ok(user) => user,
        Err(e) => {
            // Check for unique constraint violation
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("idx_users_username") {
                    return error_response(
                        StatusCode::CONFLICT,
                        "duplicate_error",
                        "A user with this username already exists",
                    );
                }
            }
            warn!(error = %e, "Failed to create user");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create user",
            );
        }
    };

    // If initial_credit is provided and greater than zero, create a credit balance change
    if let Some(ref initial_credit) = payload.initial_credit {
        if *initial_credit > BigDecimal::from(0) {
            let content = BalanceChangeContent::Credit {
                amount: initial_credit.clone(),
            };
            let unique_request_id = format!("initial-credit-{}", user.id);
            let new_change = match NewBalanceChange::from_content(
                user.id,
                unique_request_id,
                &content,
            ) {
                Ok(change) => change,
                Err(e) => {
                    warn!(error = %e, user_id = user.id, "Failed to serialize initial credit content");
                    // User was created successfully, but credit failed — still return the user
                    return (StatusCode::CREATED, Json(UserResponse::from(user))).into_response();
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
                    warn!(error = %e, user_id = user.id, "Failed to create initial credit balance change record");
                    return (StatusCode::CREATED, Json(UserResponse::from(user))).into_response();
                }
            };

            let task = BalanceChangeTask {
                balance_change_id: balance_change.id as i64,
            };
            let mut storage = state.balance_change_storage.clone();
            if let Err(e) = storage.push(task).await {
                warn!(
                    error = %e,
                    user_id = user.id,
                    balance_change_id = balance_change.id,
                    "Failed to enqueue initial credit balance change task"
                );
            }
        }
    }

    (StatusCode::CREATED, Json(UserResponse::from(user))).into_response()
}

/// GET /api/v1/users/:user_id
///
/// Returns a single user by their ID.
async fn get_user_by_id(State(state): State<Arc<AppState>>, Path(user_id): Path<i32>) -> Response {
    match db::user::get_user_by_id(&state.pool, user_id).await {
        Ok(Some(user)) => Json(UserResponse::from(user)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("User with id {} not found", user_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            )
        }
    }
}

/// GET /api/v1/users_byname/:username
///
/// Returns a single user by their username.
async fn get_user_by_username(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> Response {
    match db::user::get_user_by_username(&state.pool, &username).await {
        Ok(Some(user)) => Json(UserResponse::from(user)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("User with username '{}' not found", username),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get user by username");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            )
        }
    }
}

// --- Fund Handlers ---

/// GET /api/v1/users/:user_id/fund
///
/// Returns the fund (asset) information for a given user.
/// If the user has no fund record yet, returns a default fund with zero balances.
async fn get_user_fund(State(state): State<Arc<AppState>>, Path(user_id): Path<i32>) -> Response {
    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    match db::fund::find_user_fund(&state.pool, user_id).await {
        Ok(Some(fund)) => Json(FundResponse::from(fund)).into_response(),
        Ok(None) => {
            // User exists but has no fund record yet, return default zero balances
            Json(FundResponse {
                id: 0,
                user_id,
                cash: "0".to_string(),
                credit: "0".to_string(),
                debt: "0".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            })
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to get fund for user");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user fund",
            )
        }
    }
}

// --- AccessKey Handlers ---

/// GET /api/v1/users/:user_id/apikeys
///
/// Returns a paginated list of API keys for a given user.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_user_apikeys(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Response {
    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count of access keys for this user
    let total = match db::api::count_access_keys_by_user(&state.pool, user_id).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count access keys for user");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query access keys",
            );
        }
    };

    // Get paginated access keys
    let keys =
        match db::api::list_access_keys_by_user_paginated(&state.pool, user_id, offset, page_size)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                warn!(error = %e, "Failed to list access keys for user");
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Failed to query access keys",
                );
            }
        };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<AccessKeyResponse> = keys.into_iter().map(AccessKeyResponse::from).collect();

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

/// POST /api/v1/users/:user_id/apikeys
///
/// Creates a new API key for the specified user.
async fn create_user_apikey(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i32>,
) -> Response {
    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    match db::api::create_access_key_for_user(&state.pool, user_id).await {
        Ok(key) => (StatusCode::CREATED, Json(AccessKeyResponse::from(key))).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to create access key for user");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create access key",
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
    description: String,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::OpenAIModel> for ModelResponse {
    fn from(m: crate::models::OpenAIModel) -> Self {
        Self {
            id: m.id,
            endpoint_id: m.endpoint_id,
            model_id: m.model_id,
            has_chat_completion: m.has_chat_completion,
            has_embedding: m.has_embedding,
            has_image_generation: m.has_image_generation,
            has_speech: m.has_speech,
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
                        let update = crate::models::UpdateOpenAIEndpoint {
                            name: None,
                            api_base: None,
                            api_key: None,
                            has_responses_api: None,
                            tags: if payload.tags.is_empty() { None } else { Some(payload.tags) },
                            proxies: if payload.proxies.is_empty() { None } else { Some(payload.proxies) },
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

    let update = crate::models::UpdateOpenAIEndpoint {
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
}

/// PUT /api/v1/models/:model_id
///
/// Updates an existing model. Only the provided fields will be updated.
///
/// Request body (JSON):
/// - `description` (optional): Description of the model
async fn update_model_by_id(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<i32>,
    Json(payload): Json<UpdateModelRequest>,
) -> Response {
    let update = crate::models::UpdateOpenAIModel {
        model_id: None,
        has_image_generation: None,
        has_speech: None,
        has_chat_completion: None,
        has_embedding: None,
        input_token_price: None,
        output_token_price: None,
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
    user_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a withdrawal
#[derive(Deserialize)]
struct CreateWithdrawRequest {
    user_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a credit
#[derive(Deserialize)]
struct CreateCreditRequest {
    user_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Response DTO for a balance change (deposit)
#[derive(Serialize)]
struct BalanceChangeResponse {
    id: i32,
    user_id: i32,
    unique_request_id: String,
    content: serde_json::Value,
    is_applied: bool,
    created_at: String,
}

impl From<crate::models::BalanceChange> for BalanceChangeResponse {
    fn from(bc: crate::models::BalanceChange) -> Self {
        Self {
            id: bc.id,
            user_id: bc.user_id,
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
/// Creates a deposit for a user. This creates a BalanceChange record and enqueues
/// a BalanceChangeTask to apply it asynchronously.
///
/// Request body (JSON):
/// - `user_id` (required): The ID of the user to deposit to
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

    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, payload.user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", payload.user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Deposit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.user_id,
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
/// Creates a withdrawal for a user. This first checks that the user's fund has
/// sufficient cash (cash >= amount). If not, returns an error. Otherwise, creates
/// a BalanceChange record and enqueues a BalanceChangeTask to apply it asynchronously.
///
/// Request body (JSON):
/// - `user_id` (required): The ID of the user to withdraw from
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

    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, payload.user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", payload.user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    // Check that the user's fund has sufficient cash
    match db::fund::find_user_fund(&state.pool, payload.user_id).await {
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
            // User has no fund record, so cash is effectively 0
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
            warn!(error = %e, "Failed to query user fund");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user fund",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Withdraw {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.user_id,
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
/// Creates a credit for a user. This creates a BalanceChange record and enqueues
/// a BalanceChangeTask to apply it asynchronously. Unlike deposits which add to
/// the cash field, credits add to the credit field of the user's fund.
///
/// Request body (JSON):
/// - `user_id` (required): The ID of the user to credit
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

    // Verify the user exists
    match db::user::get_user_by_id(&state.pool, payload.user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("User with id {} not found", payload.user_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get user by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query user",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Credit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.user_id,
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

// --- Router ---

pub fn get_router(pool: DbPool, balance_change_storage: RedisStorage<BalanceChangeTask>) -> Router {
    let state = Arc::new(AppState {
        pool,
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
        .route("/users", get(list_users).post(create_user))
        .route("/users/{user_id}", get(get_user_by_id))
        .route("/users/{user_id}/fund", get(get_user_fund))
        .route(
            "/users/{user_id}/apikeys",
            get(list_user_apikeys).post(create_user_apikey),
        )
        .route("/users_by_name/{username}", get(get_user_by_username))
        .route("/endpoint-tests", post(test_endpoint))
        .route("/deposits", post(create_deposit))
        .route("/withdrawals", post(create_withdraw))
        .route("/credits", post(create_credit))
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))
        .with_state(state)
}
