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

use crate::db::{self, DbPool};
use crate::defer::BalanceChangeTask;
use crate::middlewares::admin_auth;
use crate::models::{BalanceChangeContent, NewBalanceChange};
use crate::models::{Consumer, NewConsumer, UpdateConsumer};

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

// --- Consumer Response DTO ---

/// Response DTO for a consumer
#[derive(Serialize)]
struct ConsumerResponse {
    id: i32,
    name: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

impl From<Consumer> for ConsumerResponse {
    fn from(u: Consumer) -> Self {
        Self {
            id: u.id,
            name: u.name,
            is_active: u.is_active,
            created_at: u.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: u.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

/// Request body for creating a new consumer
#[derive(Deserialize)]
struct CreateConsumerRequest {
    name: String,
    /// Optional initial credit amount to add to the consumer's fund after creation.
    /// If greater than zero, a credit balance change will be created and enqueued.
    initial_credit: Option<BigDecimal>,
}

// --- Fund Response DTO ---

/// Response DTO for a consumer's fund
#[derive(Serialize)]
struct FundResponse {
    id: i32,
    consumer_id: i32,
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
            consumer_id: f.consumer_id,
            cash: f.cash.to_string(),
            credit: f.credit.to_string(),
            debt: f.debt.to_string(),
            created_at: f.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            updated_at: f.updated_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }
}

// --- OpenAIAPIKey Response DTO ---

/// Response DTO for an API key
#[derive(Serialize)]
struct OpenAIAPIKeyResponse {
    id: i32,
    consumer_id: Option<i32>,
    apikey: String,
    label: String,
    is_active: bool,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<crate::models::OpenAIAPIKey> for OpenAIAPIKeyResponse {
    fn from(ak: crate::models::OpenAIAPIKey) -> Self {
        Self {
            id: ak.id,
            consumer_id: ak.consumer_id,
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

// --- User Handlers ---

/// GET /api/v1/consumers
///
/// Returns a paginated list of consumers.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_consumers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Response {
    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count
    let total = match db::consumer::count_consumers(&state.pool).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count consumers");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumers",
            );
        }
    };

    // Get paginated consumers
    let consumers = match db::consumer::list_consumers_paginated(&state.pool, offset, page_size).await {
        Ok(consumers) => consumers,
        Err(e) => {
            warn!(error = %e, "Failed to list consumers");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumers",
            );
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + page_size - 1) / page_size
    };

    let data: Vec<ConsumerResponse> = consumers.into_iter().map(ConsumerResponse::from).collect();

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

/// POST /api/v1/consumers
///
/// Creates a new consumer.
///
/// Request body (JSON):
/// - `name` (required): The name for the new consumer
/// - `initial_credit` (optional): Initial credit amount to add to the consumer's fund.
///   If greater than zero, a credit balance change will be created and applied asynchronously.
async fn create_consumer(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateConsumerRequest>,
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

    let new_consumer = NewConsumer {
        name: payload.name.trim().to_string(),
    };

    let consumer = match db::consumer::create_consumer(&state.pool, &new_consumer).await {
        Ok(consumer) => consumer,
        Err(e) => {
            // Check for unique constraint violation
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("idx_consumers_name") {
                    return error_response(
                        StatusCode::CONFLICT,
                        "duplicate_error",
                        "A consumer with this name already exists",
                    );
                }
            }
            warn!(error = %e, "Failed to create consumer");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create consumer",
            );
        }
    };

    // If initial_credit is provided and greater than zero, create a credit balance change
    if let Some(ref initial_credit) = payload.initial_credit {
        if *initial_credit > BigDecimal::from(0) {
            let content = BalanceChangeContent::Credit {
                amount: initial_credit.clone(),
            };
            let unique_request_id = format!("initial-credit-{}", consumer.id);
            let new_change = match NewBalanceChange::from_content(
                consumer.id,
                unique_request_id,
                &content,
            ) {
                Ok(change) => change,
                Err(e) => {
                    warn!(error = %e, consumer_id = consumer.id, "Failed to serialize initial credit content");
                    return (StatusCode::CREATED, Json(ConsumerResponse::from(consumer))).into_response();
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
                    warn!(error = %e, consumer_id = consumer.id, "Failed to create initial credit balance change record");
                    return (StatusCode::CREATED, Json(ConsumerResponse::from(consumer))).into_response();
                }
            };

            let task = BalanceChangeTask {
                balance_change_id: balance_change.id as i64,
            };
            let mut storage = state.balance_change_storage.clone();
            if let Err(e) = storage.push(task).await {
                warn!(
                    error = %e,
                    consumer_id = consumer.id,
                    balance_change_id = balance_change.id,
                    "Failed to enqueue initial credit balance change task"
                );
            }
        }
    }

    (StatusCode::CREATED, Json(ConsumerResponse::from(consumer))).into_response()
}

/// GET /api/v1/consumers/:consumer_id
///
/// Returns a single consumer by their ID.
async fn get_consumer_by_id(State(state): State<Arc<AppState>>, Path(consumer_id): Path<i32>) -> Response {
    match db::consumer::get_consumer_by_id(&state.pool, consumer_id).await {
        Ok(Some(consumer)) => Json(ConsumerResponse::from(consumer)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Consumer with id {} not found", consumer_id),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            )
        }
    }
}

/// Request body for updating a user
#[derive(Deserialize)]
struct UpdateConsumerRequest {
    name: Option<String>,
    is_active: Option<bool>,
}

/// PUT /api/v1/consumers/:consumer_id
///
/// Updates an existing consumer. Only the provided fields will be updated.
///
/// Request body (JSON):
/// - `name` (optional): New name for the consumer
/// - `is_active` (optional): Whether the consumer is active
async fn update_consumer_by_id(
    State(state): State<Arc<AppState>>,
    Path(consumer_id): Path<i32>,
    Json(payload): Json<UpdateConsumerRequest>,
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

    let update = UpdateConsumer {
        name: payload.name.map(|u| u.trim().to_string()),
        is_active: payload.is_active,
    };

    match db::consumer::update_consumer(&state.pool, consumer_id, &update).await {
        Ok(consumer) => Json(ConsumerResponse::from(consumer)).into_response(),
        Err(sqlx::Error::RowNotFound) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Consumer with id {} not found", consumer_id),
        ),
        Err(e) => {
            // Check for unique constraint violation
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("idx_consumers_name") {
                    return error_response(
                        StatusCode::CONFLICT,
                        "duplicate_error",
                        "A consumer with this name already exists",
                    );
                }
            }
            warn!(error = %e, "Failed to update consumer");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to update consumer",
            )
        }
    }
}

/// GET /api/v1/consumers_by_name/:name
///
/// Returns a single consumer by their name.
async fn get_consumer_by_name(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match db::consumer::get_consumer_by_name(&state.pool, &name).await {
        Ok(Some(consumer)) => Json(ConsumerResponse::from(consumer)).into_response(),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("Consumer with name '{}' not found", name),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by name");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            )
        }
    }
}

// --- Fund Handlers ---

/// GET /api/v1/consumers/:consumer_id/fund
///
/// Returns the fund (asset) information for a given consumer.
/// If the consumer has no fund record yet, returns a default fund with zero balances.
async fn get_consumer_fund(State(state): State<Arc<AppState>>, Path(consumer_id): Path<i32>) -> Response {
    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    match db::fund::find_consumer_fund(&state.pool, consumer_id).await {
        Ok(Some(fund)) => Json(FundResponse::from(fund)).into_response(),
        Ok(None) => {
            // Consumer exists but has no fund record yet, return default zero balances
            Json(FundResponse {
                id: 0,
                consumer_id: consumer_id,
                cash: "0".to_string(),
                credit: "0".to_string(),
                debt: "0".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            })
            .into_response()
        }
        Err(e) => {
            warn!(error = %e, "Failed to get fund for consumer");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer fund",
            )
        }
    }
}

// --- AccessKey Handlers ---

/// GET /api/v1/consumers/:consumer_id/apikeys
///
/// Returns a paginated list of API keys for a given consumer.
///
/// Query parameters:
/// - `page` (optional, default: 1): Page number (1-based)
/// - `page_size` (optional, default: 20, max: 100): Number of items per page
async fn list_consumer_apikeys(
    State(state): State<Arc<AppState>>,
    Path(consumer_id): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Response {
    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    let page = if params.page < 1 { 1 } else { params.page };
    let page_size = params.page_size.clamp(1, MAX_PAGE_SIZE);
    let offset = (page - 1) * page_size;

    // Get total count of API keys for this consumer
    let total = match db::api::count_api_keys_by_consumer(&state.pool, consumer_id).await {
        Ok(count) => count,
        Err(e) => {
            warn!(error = %e, "Failed to count API keys for consumer");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query API keys",
            );
        }
    };

    // Get paginated API keys
    let keys =
        match db::api::list_api_keys_by_consumer_paginated(&state.pool, consumer_id, offset, page_size)
            .await
        {
            Ok(keys) => keys,
            Err(e) => {
                warn!(error = %e, "Failed to list API keys for consumer");
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

    let data: Vec<OpenAIAPIKeyResponse> =
        keys.into_iter().map(OpenAIAPIKeyResponse::from).collect();

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

/// POST /api/v1/consumers/:consumer_id/apikeys
///
/// Creates a new API key for the specified consumer.
///
/// Request body (JSON):
/// - `label` (optional): A label describing the purpose of this API key
async fn create_consumer_apikey(
    State(state): State<Arc<AppState>>,
    Path(consumer_id): Path<i32>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Response {
    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    match db::api::create_api_key_for_consumer(&state.pool, consumer_id, &payload.label).await {
        Ok(key) => (StatusCode::CREATED, Json(OpenAIAPIKeyResponse::from(key))).into_response(),
        Err(e) => {
            warn!(error = %e, "Failed to create API key for consumer");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to create API key",
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
                        let update = crate::models::UpdateOpenAIEndpoint {
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

    let update = crate::models::UpdateOpenAIModel {
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
    consumer_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a withdrawal
#[derive(Deserialize)]
struct CreateWithdrawRequest {
    consumer_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Request body for creating a credit
#[derive(Deserialize)]
struct CreateCreditRequest {
    consumer_id: i32,
    unique_request_id: String,
    amount: BigDecimal,
}

/// Response DTO for a balance change (deposit)
#[derive(Serialize)]
struct BalanceChangeResponse {
    id: i32,
    consumer_id: i32,
    unique_request_id: String,
    content: serde_json::Value,
    is_applied: bool,
    created_at: String,
}

impl From<crate::models::BalanceChange> for BalanceChangeResponse {
    fn from(bc: crate::models::BalanceChange) -> Self {
        Self {
            id: bc.id,
            consumer_id: bc.consumer_id,
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
/// - `consumer_id` (required): The ID of the consumer to deposit to
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

    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, payload.consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", payload.consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Deposit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.consumer_id,
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
/// - `consumer_id` (required): The ID of the consumer to withdraw from
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

    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, payload.consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", payload.consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    // Check that the consumer's fund has sufficient cash
    match db::fund::find_consumer_fund(&state.pool, payload.consumer_id).await {
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
        payload.consumer_id,
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
/// - `consumer_id` (required): The ID of the consumer to credit
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

    // Verify the consumer exists
    match db::consumer::get_consumer_by_id(&state.pool, payload.consumer_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                &format!("Consumer with id {} not found", payload.consumer_id),
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to get consumer by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "Failed to query consumer",
            );
        }
    }

    // Create the BalanceChange content
    let content = BalanceChangeContent::Credit {
        amount: payload.amount.clone(),
    };
    let new_change = match NewBalanceChange::from_content(
        payload.consumer_id,
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
        .route("/consumers", get(list_consumers).post(create_consumer))
        .route(
            "/consumers/{consumer_id}",
            get(get_consumer_by_id).put(update_consumer_by_id),
        )
        .route("/consumers/{consumer_id}/fund", get(get_consumer_fund))
        .route(
            "/consumers/{consumer_id}/apikeys",
            get(list_consumer_apikeys).post(create_consumer_apikey),
        )
        .route("/consumers_by_name/{name}", get(get_consumer_by_name))
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
        .route("/deposits", post(create_deposit))
        .route("/withdrawals", post(create_withdraw))
        .route("/credits", post(create_credit))
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))
        .with_state(state)
}
