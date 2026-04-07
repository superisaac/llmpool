use apalis::prelude::*;
use apalis_cron::CronStream;
use cron::Schedule;
use std::str::FromStr;
use tracing::info;

use crate::db;
use crate::telemetry;

/// Cron expression: every 5 minutes
const UPSTREAM_HEALTH_CHECK_CRON: &str = "0 */5 * * * *";

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

    let schedule = Schedule::from_str(UPSTREAM_HEALTH_CHECK_CRON)
        .expect("Invalid cron expression for upstream health check");

    let pool_clone = pool.clone();
    let redis_pool_clone = redis_pool.clone();
    let balance_change_storage_clone = balance_change_storage.clone();
    let health_check_pool = pool.clone();

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
        .register(move |_| {
            WorkerBuilder::new("upstream-health-worker")
                .backend(CronStream::new(schedule.clone()))
                .data(health_check_pool.clone())
                .build(super::tasks::check_offline_upstreams)
        })
        .run()
        .await
        .expect("Failed to run deferred task worker");
}
