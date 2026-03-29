use apalis::prelude::*;
use tracing::info;

use crate::db;
use crate::telemetry;

/// Start the deferred task queue worker with the given concurrency level.
pub async fn run_worker(concurrency: usize) {
    // Initialize tracing for the worker
    let _tracer_provider = telemetry::init_telemetry();

    let pool = db::create_pool_from_config().await;
    let redis_pool = db::create_redis_pool_from_config().await;
    let event_storage = super::create_event_storage().await;
    let balance_change_storage = super::create_balance_change_storage().await;

    info!(
        concurrency = concurrency,
        "Starting deferred task queue worker"
    );

    // Clean up stale worker entries in Redis to prevent
    // "worker is still active within threshold" errors on restart
    super::cleanup_stale_workers(&["event-worker", "balance-change-worker"]).await;

    let pool_clone = pool.clone();
    let redis_pool_clone = redis_pool.clone();
    let balance_change_storage_clone = balance_change_storage.clone();
    Monitor::new()
        .register(move |_| {
            WorkerBuilder::new("event-worker")
                .backend(event_storage.clone())
                .data(pool.clone())
                .data(redis_pool.clone())
                .data(balance_change_storage_clone.clone())
                .concurrency(concurrency)
                .build(super::tasks::handle_openai_event)
        })
        .register(move |_| {
            WorkerBuilder::new("balance-change-worker")
                .backend(balance_change_storage.clone())
                .data(pool_clone.clone())
                .data(redis_pool_clone.clone())
                .concurrency(concurrency)
                .build(super::tasks::settle_balance_change)
        })
        .run()
        .await
        .expect("Failed to run deferred task worker");
}
