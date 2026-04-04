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

/// Build a reqwest::Client with an optional random proxy from the upstream's proxies list.
fn build_http_client_for_upstream(
    upstream: &crate::models::LLMUpstream,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder();
    if !upstream.proxies.is_empty() {
        let mut rng = rand::rng();
        if let Some(proxy_url) = upstream.proxies.choose(&mut rng) {
            info!(
                upstream_name = %upstream.name,
                proxy = %proxy_url,
                "Passthrough: using proxy for upstream"
            );
            let proxy = reqwest::Proxy::all(proxy_url.as_str()).expect("Invalid proxy URL");
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

/// Proxy the request to the given upstream, rewriting the URL to /{rest}.
async fn proxy_to_upstream(
    _state: &PassthroughState,
    upstream: &crate::models::LLMUpstream,
    rest: &str,
    req: Request,
) -> Response {
    // Build the upstream URL: {api_base}/{rest}
    let upstream_url = format!(
        "{}/{}",
        upstream.api_base.trim_end_matches('/'),
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
    let http_client = match build_http_client_for_upstream(upstream) {
        Ok(client) => client,
        Err(e) => {
            error!(
                upstream_name = %upstream.name,
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

    // Set Authorization header from upstream's api_key (decrypted) if it's not empty
    if !upstream.api_key.is_empty() {
        upstream_req = upstream_req.header("Authorization", format!("Bearer {}", upstream.api_key));
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

/// Passthrough handler: proxies the request to a randomly selected upstream matching the tag.
/// URL pattern: /passthrough/tag/:tag/*rest
/// The upstream URL is rewritten to: {upstream.api_base}/{rest}
async fn passthrough_by_tag_handler(
    State(state): State<Arc<PassthroughState>>,
    Path((tag, rest)): Path<(String, String)>,
    req: Request,
) -> Response {
    // 1. Find upstreams by tag
    let upstreams = match db::llm::find_upstreams_by_tag(&state.pool, &tag).await {
        Ok(eps) => eps,
        Err(e) => {
            warn!(tag = %tag, error = %e, "Failed to query upstreams by tag");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to query upstreams.",
            );
        }
    };

    if upstreams.is_empty() {
        return error_response(
            StatusCode::NOT_FOUND,
            &format!("No upstreams found for tag '{}'", tag),
        );
    }

    // 2. Randomly select one upstream
    let upstream = {
        let mut rng = rand::rng();
        upstreams.choose(&mut rng).unwrap()
    };

    info!(
        tag = %tag,
        upstream_name = %upstream.name,
        api_base = %upstream.api_base,
        rest = %rest,
        "Passthrough: selected upstream by tag"
    );

    proxy_to_upstream(&state, upstream, &rest, req).await
}

/// Passthrough handler: proxies the request to a specific upstream by its ID.
/// URL pattern: /passthrough/:upstream_id/*rest
/// The upstream URL is rewritten to: {upstream.api_base}/{rest}
async fn passthrough_by_upstream_id_handler(
    State(state): State<Arc<PassthroughState>>,
    Path((upstream_id, rest)): Path<(i32, String)>,
    req: Request,
) -> Response {
    // 1. Find upstream by ID
    let upstream = match db::llm::get_upstream(&state.pool, upstream_id).await {
        Ok(ep) => ep,
        Err(sqlx::Error::RowNotFound) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Upstream with id '{}' not found", upstream_id),
            );
        }
        Err(e) => {
            warn!(upstream_id = %upstream_id, error = %e, "Failed to query upstream by id");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to query upstream.",
            );
        }
    };

    info!(
        upstream_id = %upstream_id,
        upstream_name = %upstream.name,
        api_base = %upstream.api_base,
        rest = %rest,
        "Passthrough: selected upstream by id"
    );

    proxy_to_upstream(&state, &upstream, &rest, req).await
}

// --- Router ---

pub fn get_router(pool: DbPool) -> Router {
    let state = Arc::new(PassthroughState { pool });

    Router::new()
        .route("/tag/{tag}/{*rest}", any(passthrough_by_tag_handler))
        .route(
            "/{upstream_id}/{*rest}",
            any(passthrough_by_upstream_id_handler),
        )
        .route_layer(middleware::from_fn(admin_auth::auth_jwt))
        .with_state(state)
}
