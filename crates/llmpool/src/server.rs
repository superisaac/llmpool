use axum::{Router, middleware};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::middlewares::rate_limit::{RateLimitState, rate_limit_middleware};
use crate::provider::get_all_providers;
use crate::provider::types::ProviderContext;

use crate::telemetry;
use crate::views::admin_rest_api;
use crate::views::passthrough;

pub async fn serve(bind: &str) {
    // Initialize OpenTelemetry tracing
    let tracer_provider = telemetry::init_telemetry();

    let pool = crate::db::create_pool_from_config().await;
    let redis_pool = crate::db::create_redis_pool_from_config().await;
    let openai_event_storage = crate::defer::create_event_storage().await;
    let anthropic_event_storage = crate::defer::create_anthropic_event_storage().await;
    let balance_change_storage = crate::defer::create_balance_change_storage().await;

    // Build the rate limiting state (shares the same Redis pool)
    let rate_limit_state = Arc::new(RateLimitState {
        redis_pool: redis_pool.clone(),
    });

    // Build the shared provider context (holds all runtime dependencies)
    let provider_ctx = ProviderContext {
        pool: pool.clone(),
        redis_pool: redis_pool.clone(),
        openai_event_storage,
        anthropic_event_storage,
    };

    let admin_rest_router =
        admin_rest_api::get_router(pool.clone(), redis_pool, balance_change_storage);
    let passthrough_router = passthrough::get_router(pool);

    // Mount each provider's router under its declared prefix.
    // The OpenAI provider additionally gets a rate-limiting layer.
    let mut app = Router::new()
        .nest("/api/v1", admin_rest_router)
        .nest("/passthrough", passthrough_router);

    for provider in get_all_providers() {
        let prefix = provider.get_router_prefix().to_string();
        let router = provider.get_router(&provider_ctx);

        let router = if provider.provider_name() == "openai" {
            // Apply rate limiting to the OpenAI provider before CORS so that
            // rate-limited requests are rejected early, before any CORS headers
            // are added.
            router
                .route_layer(middleware::from_fn_with_state(
                    rate_limit_state.clone(),
                    rate_limit_middleware,
                ))
                .layer(CorsLayer::very_permissive())
        } else {
            router.layer(CorsLayer::very_permissive())
        };

        info!(prefix = %prefix, provider = %provider.provider_name(), "Mounting provider router");
        app = app.nest(&prefix, router);
    }

    let app = app.layer(TraceLayer::new_for_http());

    // Parse bind address from CLI argument
    let addr: SocketAddr = bind
        .parse()
        .expect("Invalid bind address, expected format: HOST:PORT");

    info!("LLMPool proxy running at: http://{}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    // Use into_make_service_with_connect_info so that ConnectInfo<SocketAddr>
    // is available in middleware for IP-based rate limiting.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();

    // Gracefully shut down telemetry on exit
    telemetry::shutdown_telemetry(tracer_provider);
}
