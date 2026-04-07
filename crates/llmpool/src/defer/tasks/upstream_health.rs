use std::time::Duration;

use apalis::prelude::Data;
use apalis_cron::Tick;
use tracing::{info, warn};

use crate::db::{self, DbPool};

/// Check a single offline upstream by calling its /models endpoint.
/// Returns true if the upstream is reachable (HTTP 200), false otherwise.
async fn check_upstream_health(api_base: &str, api_key: &str) -> bool {
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    match client.get(&url).bearer_auth(api_key).send().await {
        Ok(resp) => {
            let status = resp.status();
            info!(
                url = %url,
                status = status.as_u16(),
                "Upstream health check response"
            );
            status.is_success()
        }
        Err(e) => {
            warn!(
                url = %url,
                error = %e,
                "Upstream health check failed"
            );
            false
        }
    }
}

/// Check all offline upstreams concurrently and mark them online if reachable.
/// This is the apalis-cron handler, called every 5 minutes.
pub async fn check_offline_upstreams(_tick: Tick, pool: Data<DbPool>) {
    let upstreams = match db::llm::list_offline_upstreams(&pool).await {
        Ok(list) => list,
        Err(e) => {
            warn!(error = %e, "Failed to list offline upstreams");
            return;
        }
    };

    if upstreams.is_empty() {
        return;
    }

    info!(count = upstreams.len(), "Checking offline upstreams");

    // Spawn concurrent health checks for all offline upstreams
    let mut handles = Vec::with_capacity(upstreams.len());
    for upstream in upstreams {
        let pool_clone = (*pool).clone();
        let handle = tokio::spawn(async move {
            let is_healthy = check_upstream_health(&upstream.api_base, &upstream.api_key).await;
            if is_healthy {
                match db::llm::mark_upstream_online(&pool_clone, upstream.id).await {
                    Ok(()) => {
                        info!(
                            upstream_id = upstream.id,
                            upstream_name = %upstream.name,
                            "Upstream is back online"
                        );
                    }
                    Err(e) => {
                        warn!(
                            upstream_id = upstream.id,
                            error = %e,
                            "Failed to mark upstream as online"
                        );
                    }
                }
            } else {
                info!(
                    upstream_id = upstream.id,
                    upstream_name = %upstream.name,
                    "Upstream is still offline"
                );
            }
        });
        handles.push(handle);
    }

    // Wait for all checks to complete
    for handle in handles {
        if let Err(e) = handle.await {
            warn!(error = %e, "Upstream health check task panicked");
        }
    }
}
