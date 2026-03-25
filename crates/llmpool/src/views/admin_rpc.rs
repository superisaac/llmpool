use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use axum_jrpc::{
    JrpcResult, JsonRpcExtractor, JsonRpcResponse,
    error::{JsonRpcError, JsonRpcErrorReason},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::warn;

use crate::config;
use crate::db::{self, DbPool};
use crate::models::NewUser;

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

// --- JWT Auth Middleware ---

/// Middleware that authenticates admin API requests using JWT Bearer token.
async fn auth_jwt(request: Request, next: Next) -> Response {
    let cfg = config::get_config();
    let jwt_secret = &cfg.admin.jwt_secret;

    if jwt_secret.is_empty() {
        warn!("Admin JWT secret is not configured");
        let error = JsonRpcError::new(
            JsonRpcErrorReason::InternalError,
            "Admin API is not configured".to_string(),
            Value::Null,
        );
        return Json(JsonRpcResponse::error(axum_jrpc::Id::None(()), error)).into_response();
    }

    // Extract the Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            let error = JsonRpcError::new(
                JsonRpcErrorReason::ServerError(-32000),
                "Missing or invalid Authorization header. Expected: Bearer <jwt_token>".to_string(),
                Value::Null,
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(JsonRpcResponse::error(axum_jrpc::Id::None(()), error)),
            )
                .into_response();
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
            let error = JsonRpcError::new(
                JsonRpcErrorReason::ServerError(-32000),
                "Invalid JWT token".to_string(),
                Value::Null,
            );
            (
                StatusCode::UNAUTHORIZED,
                Json(JsonRpcResponse::error(axum_jrpc::Id::None(()), error)),
            )
                .into_response()
        }
    }
}

// --- JSON-RPC Handler ---

async fn handle_jsonrpc(State(state): State<Arc<AppState>>, req: JsonRpcExtractor) -> JrpcResult {
    let method = req.method().to_string();

    match method.as_str() {
        "getBalance" => handle_get_balance(&state.pool, req).await,
        "createUser" => handle_create_user(&state.pool, req).await,
        "createApiKey" => handle_create_api_key(&state.pool, req).await,
        m => Ok(req.method_not_found(m)),
    }
}

// --- Helper to create error responses ---

fn jrpc_error(id: axum_jrpc::Id, reason: JsonRpcErrorReason, message: String) -> JsonRpcResponse {
    let error = JsonRpcError::new(reason, message, Value::Null);
    JsonRpcResponse::error(id, error)
}

// --- RPC Method Handlers ---

/// getBalance: Get user balance information
/// Params: { "user_id": i32 }
/// Returns: { "cash": decimal, "debt": decimal }
async fn handle_get_balance(pool: &DbPool, req: JsonRpcExtractor) -> JrpcResult {
    let req_id = req.get_answer_id();

    #[derive(Deserialize)]
    struct Params {
        user_id: i32,
    }

    let params: Params = req.parse_params()?;

    // Verify user exists
    db::api::find_user_by_id(pool, params.user_id)
        .await
        .map_err(|e| {
            warn!(error = %e, "Database error looking up user");
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InternalError,
                "Internal database error".to_string(),
            )
        })?
        .ok_or_else(|| {
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InvalidParams,
                format!("User with id {} not found", params.user_id),
            )
        })?;

    // Get user balance
    let balance = db::balance::find_user_balance(pool, params.user_id)
        .await
        .map_err(|e| {
            warn!(error = %e, "Database error looking up user balance");
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InternalError,
                "Internal database error".to_string(),
            )
        })?;

    let result = match balance {
        Some(b) => json!({
            "cash": b.cash.to_string(),
            "credit": b.credit.to_string(),
            "debt": b.debt.to_string(),
        }),
        None => json!({
            "cash": "0",
            "credit": "0",
            "debt": "0",
        }),
    };

    Ok(JsonRpcResponse::success(req_id, result))
}

/// createUser: Create a new user
/// Params: { "username": string }
/// Returns: { "user_id": i32 }
async fn handle_create_user(pool: &DbPool, req: JsonRpcExtractor) -> JrpcResult {
    let req_id = req.get_answer_id();

    #[derive(Deserialize)]
    struct Params {
        username: String,
    }

    let params: Params = req.parse_params()?;

    if params.username.is_empty() {
        return Err(jrpc_error(
            req_id,
            JsonRpcErrorReason::InvalidParams,
            "Username cannot be empty".to_string(),
        ));
    }

    let new_user = NewUser {
        username: params.username,
    };

    let user = db::user::create_user(pool, &new_user).await.map_err(|e| {
        warn!(error = %e, "Database error creating user");
        jrpc_error(
            req_id.clone(),
            JsonRpcErrorReason::InternalError,
            format!("Failed to create user: {}", e),
        )
    })?;

    Ok(JsonRpcResponse::success(
        req_id,
        json!({
            "user_id": user.id,
        }),
    ))
}

/// createApiKey: Create a new API key for a user
/// Params: { "user_id": i32 }
/// Returns: { "api_key": string }
async fn handle_create_api_key(pool: &DbPool, req: JsonRpcExtractor) -> JrpcResult {
    let req_id = req.get_answer_id();

    #[derive(Deserialize)]
    struct Params {
        user_id: i32,
    }

    let params: Params = req.parse_params()?;

    // Verify user exists
    db::api::find_user_by_id(pool, params.user_id)
        .await
        .map_err(|e| {
            warn!(error = %e, "Database error looking up user");
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InternalError,
                "Internal database error".to_string(),
            )
        })?
        .ok_or_else(|| {
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InvalidParams,
                format!("User with id {} not found", params.user_id),
            )
        })?;

    let access_key = db::api::create_access_key_for_user(pool, params.user_id)
        .await
        .map_err(|e| {
            warn!(error = %e, "Database error creating access key");
            jrpc_error(
                req_id.clone(),
                JsonRpcErrorReason::InternalError,
                format!("Failed to create API key: {}", e),
            )
        })?;

    Ok(JsonRpcResponse::success(
        req_id,
        json!({
            "api_key": access_key.apikey,
        }),
    ))
}

// --- Router ---

pub fn get_router(pool: DbPool) -> Router {
    let state = Arc::new(AppState { pool });
    Router::new()
        .route("/", post(handle_jsonrpc))
        .route_layer(middleware::from_fn(auth_jwt))
        .with_state(state)
}
