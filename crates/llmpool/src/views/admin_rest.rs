use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

use crate::config;
use crate::db::{self, DbPool};

// --- JWT Claims ---

#[derive(Debug, Deserialize)]
struct Claims {
    #[allow(dead_code)]
    sub: Option<String>,
    #[allow(dead_code)]
    exp: Option<usize>,
}

// --- Server State ---

struct AppState {
    pool: DbPool,
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

// --- JWT Auth Middleware ---

/// Middleware that authenticates admin REST API requests using JWT Bearer token.
/// Uses the JWT secret configured in the admin section.
async fn auth_jwt(request: Request, next: Next) -> Response {
    let cfg = config::get_config();
    let jwt_secret = &cfg.admin.jwt_secret;

    if jwt_secret.is_empty() {
        warn!("Admin JWT secret is not configured");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "not_configured",
            "Admin API is not configured",
        );
    }

    // Extract the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Missing or invalid Authorization header. Expected: Bearer <jwt_token>",
            );
        }
    };

    // Validate the JWT token
    let decoding_key = DecodingKey::from_secret(jwt_secret.as_bytes());
    let mut validation = Validation::default();
    // Allow tokens without exp claim for flexibility
    validation.required_spec_claims.remove("exp");
    validation.validate_exp = false;

    match decode::<Claims>(token, &decoding_key, &validation) {
        Ok(_) => next.run(request).await,
        Err(e) => {
            warn!(error = %e, "JWT validation failed");
            error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "Invalid JWT token",
            )
        }
    }
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
            expires_at: ak.expires_at.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
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

    let new_user = crate::models::NewUser {
        username: payload.username.trim().to_string(),
    };

    match db::user::create_user(&state.pool, &new_user).await {
        Ok(user) => (StatusCode::CREATED, Json(UserResponse::from(user))).into_response(),
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
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create user",
            )
        }
    }
}

/// GET /api/v1/users/:user_id
///
/// Returns a single user by their ID.
async fn get_user_by_id(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i32>,
) -> Response {
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
            // Fetch the saved endpoint to return it
            match db::openai::get_endpoint_by_api_base(&state.pool, payload.api_base.trim()).await {
                Ok(endpoint) => {
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

// --- Router ---

pub fn get_router(pool: DbPool) -> Router {
    let state = Arc::new(AppState { pool });
    Router::new()
        .route("/endpoints", get(list_endpoints).post(create_endpoint))
        .route("/models", get(list_models))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{user_id}", get(get_user_by_id))
        .route(
            "/users/{user_id}/apikeys",
            get(list_user_apikeys).post(create_user_apikey),
        )
        .route("/users_by_name/{username}", get(get_user_by_username))
        .route("/endpoint-tests", post(test_endpoint))
        .route_layer(middleware::from_fn(auth_jwt))
        .with_state(state)
}
