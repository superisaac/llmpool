use axum::{
    Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::any,
};
use rand::seq::IndexedRandom;
use std::sync::Arc;
use tracing::error;
use tracing::{info, warn};

use crate::db::{self, DbPool};
use crate::middlewares::admin_auth;

// --- Server State ---

struct PassthroughState {
    pool: DbPool,
}

// --- Helpers ---

/// Build a JSON error response
fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "passthrough_error"
            }
        })),
    )
        .into_response()
}

/// Headers that should not be forwarded between client and upstream
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "x-admin-token",
];

fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|h| h.eq_ignore_ascii_case(name))
}

// --- Handler ---

/// Build a reqwest::Client with an optional random proxy from the endpoint's proxies list.
fn build_http_client_for_endpoint(
    endpoint: &crate::models::LLMEndpoint,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder();
    if !endpoint.proxies.is_empty() {
        let mut rng = rand::rng();
        if let Some(proxy_url) = endpoint.proxies.choose(&mut rng) {
            info!(
                endpoint_name = %endpoint.name,
                proxy = %proxy_url,
                "Passthrough: using proxy for endpoint"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

/// Proxy the request to the given endpoint, rewriting the URL to /{rest}.
async fn proxy_to_endpoint(
    _state: &PassthroughState,
    endpoint: &crate::models::LLMEndpoint,
    rest: &str,
    req: Request,
) -> Response {
    // Build the upstream URL: {api_base}/{rest}
    let upstream_url = format!(
        "{}/{}",
        endpoint.api_base.trim_end_matches('/'),
        rest.trim_start_matches('/')
    );

    // Append query string if present
    let upstream_url = if let Some(query) = req.uri().query() {
        format!("{}?{}", upstream_url, query)
    } else {
        upstream_url
    };

    // Extract method, headers, and body from the incoming request
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    // Build a per-request HTTP client with optional proxy
    let http_client = match build_http_client_for_endpoint(endpoint) {
        Ok(client) => client,
        Err(e) => {
            error!(
                endpoint_name = %endpoint.name,
                error = %e,
                "Passthrough: failed to build HTTP client with proxy"
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to build HTTP client: {}", e),
            );
        }
    };

    // Build the upstream request
    let reqwest_method = match method {
        Method::GET => reqwest::Method::GET,
        Method::POST => reqwest::Method::POST,
        Method::PUT => reqwest::Method::PUT,
        Method::DELETE => reqwest::Method::DELETE,
        Method::PATCH => reqwest::Method::PATCH,
        Method::HEAD => reqwest::Method::HEAD,
        Method::OPTIONS => reqwest::Method::OPTIONS,
        _ => {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                &format!("Unsupported HTTP method: {}", method),
            );
        }
    };

    let mut upstream_req = http_client.request(reqwest_method, &upstream_url);

    // Forward headers (skip hop-by-hop headers)
    for (name, value) in headers.iter() {
        if !is_hop_by_hop(name.as_str()) {
            if let Ok(val_str) = value.to_str() {
                upstream_req = upstream_req.header(name.as_str(), val_str);
            }
        }
    }

    // Set Authorization header from endpoint's api_key if it's not empty
    if !endpoint.api_key.is_empty() {
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", endpoint.api_key));
    }

    // Forward the body as a stream
    let body_stream = body.into_data_stream();
    let reqwest_body = reqwest::Body::wrap_stream(body_stream);
    upstream_req = upstream_req.body(reqwest_body);

    // Send the request to upstream
    let upstream_resp = match upstream_req.send().await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(
                upstream_url = %upstream_url,
                error = %e,
                "Passthrough: upstream request failed"
            );
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Upstream request failed: {}", e),
            );
        }
    };

    // Build the response back to the client
    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response_headers = HeaderMap::new();
    for (name, value) in upstream_resp.headers().iter() {
        if !is_hop_by_hop(name.as_str()) {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                response_headers.insert(header_name, header_value);
            }
        }
    }

    // Stream the response body back
    let upstream_body_stream = upstream_resp.bytes_stream();
    let body = Body::from_stream(upstream_body_stream);

    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    response
}

/// Passthrough handler: proxies the request to a randomly selected endpoint matching the tag.
/// URL pattern: /passthrough/tag/:tag/*rest
/// The upstream URL is rewritten to: {endpoint.api_base}/{rest}
async fn passthrough_by_tag_handler(
    State(state): State<Arc<PassthroughState>>,
    Path((tag, rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    // 1. Find endpoints by tag
    let endpoints = match db::openai::find_endpoints_by_tag(&state.pool, &tag).await {
        Ok(eps) => eps,
        Err(e) => {
            warn!(tag = %tag, error = %e, "Failed to query endpoints by tag");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to query endpoints.",
            );
        }
    };

    if endpoints.is_empty() {
        return error_response(
            StatusCode::NOT_FOUND,
            &format!("No endpoints found for tag '{}'", tag),
        );
    }

    // 2. Randomly select one endpoint
    let endpoint = {
        let mut rng = rand::rng();
        endpoints.choose(&mut rng).unwrap()
    };

    info!(
        tag = %tag,
        endpoint_name = %endpoint.name,
        api_base = %endpoint.api_base,
        rest = %rest,
        "Passthrough: selected endpoint by tag"
    );

    proxy_to_endpoint(&state, endpoint, &rest, req).await
}

/// Passthrough handler: proxies the request to a specific endpoint by its ID.
/// URL pattern: /passthrough/:endpoint_id/*rest
/// The upstream URL is rewritten to: {endpoint.api_base}/{rest}
async fn passthrough_by_endpoint_id_handler(
    State(state): State<Arc<PassthroughState>>,
    Path((endpoint_id, rest)): Path<(i32, String)>,
    req: Request,
) -> Response {
    // 1. Find endpoint by ID
    let endpoint = match db::openai::get_endpoint(&state.pool, endpoint_id).await {
        Ok(ep) => ep,
        Err(sqlx::Error::RowNotFound) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Endpoint with id '{}' not found", endpoint_id),
            );
        }
        Err(e) => {
            warn!(endpoint_id = %endpoint_id, error = %e, "Failed to query endpoint by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to query endpoint.",
            );
        }
    };

    info!(
        endpoint_id = %endpoint_id,
        endpoint_name = %endpoint.name,
        api_base = %endpoint.api_base,
        rest = %rest,
        "Passthrough: selected endpoint by id"
    );

    proxy_to_endpoint(&state, &endpoint, &rest, req).await
}

// --- Router ---

pub fn get_router(pool: DbPool) -> Router {
    let state = Arc::new(PassthroughState { pool });

    Router::new()
        .route("/tag/{tag}/{*rest}", any(passthrough_by_tag_handler))
        .route(
            "/{endpoint_id}/{*rest}",
            any(passthrough_by_endpoint_id_handler),
        )
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))
        .with_state(state)
}
