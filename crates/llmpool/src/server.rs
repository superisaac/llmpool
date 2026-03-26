use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use std::net::SocketAddr;
use tokio::net::TcpListener;

// use crate::openai;
use crate::telemetry;
use crate::views::admin_rest;
use crate::views::openai_proxy;
use crate::views::passthrough;

pub async fn serve(bind: &str) {
    // Initialize OpenTelemetry tracing
    let tracer_provider = telemetry::init_telemetry();

    let pool = crate::db::create_pool_from_config().await;
    let event_storage = crate::defer::create_event_storage().await;
    let balance_change_storage = crate::defer::create_balance_change_storage().await;

    let openai_router = openai_proxy::get_router(pool.clone(), event_storage);
    let admin_rest_router = admin_rest::get_router(pool.clone(), balance_change_storage);
    let passthrough_router = passthrough::get_router(pool);
    // Route configuration
    // Note: we can directly destructure async_openai types as Axum Json extractor inputs
    let app = Router::new()
        .nest(
            "/openai/v1",
            openai_router.layer(CorsLayer::very_permissive()),
        )
        .nest("/api/v1", admin_rest_router)
        .nest("/passthrough", passthrough_router)
        .layer(TraceLayer::new_for_http());

    // Parse bind address from CLI argument
    let addr: SocketAddr = bind
        .parse()
        .expect("Invalid bind address, expected format: HOST:PORT");

    info!(
        "OpenAI Proxy (via async-openai) running at: http://{}",
        addr
    );

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    // Gracefully shut down telemetry on exit
    telemetry::shutdown_telemetry(tracer_provider);
}
