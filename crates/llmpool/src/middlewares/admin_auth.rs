use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::Deserialize;
use tracing::warn;

use crate::config;

// --- JWT Claims ---

#[derive(Debug, Deserialize)]
struct Claims {
    #[allow(dead_code)]
    sub: Option<String>,
    #[allow(dead_code)]
    exp: Option<usize>,
    realm: Option<String>,
}

/// Build a JSON error response (used internally by the middleware)
fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "auth_error"
            }
        })),
    )
        .into_response()
}

// --- JWT Auth Middleware ---

/// Middleware that authenticates requests using JWT token
/// from the `x-admin-token` header. Uses the JWT secret configured in the admin section.
pub async fn auth_jwt(request: Request, next: Next) -> Response {
    let cfg = config::get_config();
    let jwt_secret = &cfg.admin.jwt_secret;

    if jwt_secret.is_empty() {
        warn!("Admin JWT secret is not configured");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Admin API is not configured",
        );
    }

    // Extract the x-admin-token header
    let token = match request
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
    {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Missing or invalid x-admin-token header. Expected: x-admin-token: <jwt_token>",
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
        Ok(token_data) => {
            // Check that the realm claim is "api"
            match token_data.claims.realm.as_deref() {
                Some("api") => next.run(request).await,
                _ => {
                    warn!("JWT token has invalid or missing realm claim");
                    error_response(
                        StatusCode::UNAUTHORIZED,
                        "Invalid JWT token: realm must be 'api'",
                    )
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "JWT validation failed");
            error_response(StatusCode::UNAUTHORIZED, "Invalid JWT token")
        }
    }
}
