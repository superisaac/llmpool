use crate::db::DbPool;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct BatchMeta {
    pub id: i64,
    pub batch_id: String,
    pub original_batch_id: String,
    pub upstream_id: i64,
    pub status: String,
    pub provider: String,
    pub created_at: NaiveDateTime,
}

// ── DB operations ─────────────────────────────────────────────────────────────

/// Insert a new BatchMeta record with a specific provider.
pub async fn create_batch_meta_with_provider(
    pool: &DbPool,
    batch_id: &str,
    original_batch_id: &str,
    upstream_id: i64,
    provider: &str,
) -> Result<BatchMeta, sqlx::Error> {
    sqlx::query_as::<_, BatchMeta>(
        "INSERT INTO batch_metas (batch_id, original_batch_id, upstream_id, provider)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(batch_id)
    .bind(original_batch_id)
    .bind(upstream_id)
    .bind(provider)
    .fetch_one(pool)
    .await
}

/// Look up a BatchMeta by our internal batch_id.
pub async fn get_batch_meta_by_batch_id(
    pool: &DbPool,
    batch_id: &str,
) -> Result<Option<BatchMeta>, sqlx::Error> {
    sqlx::query_as::<_, BatchMeta>("SELECT * FROM batch_metas WHERE batch_id = $1")
        .bind(batch_id)
        .fetch_optional(pool)
        .await
}

/// Update the status of a BatchMeta by our internal batch_id.
pub async fn update_batch_meta_status(
    pool: &DbPool,
    batch_id: &str,
    status: &str,
) -> Result<Option<BatchMeta>, sqlx::Error> {
    sqlx::query_as::<_, BatchMeta>(
        "UPDATE batch_metas
         SET status = $2
         WHERE batch_id = $1
         RETURNING *",
    )
    .bind(batch_id)
    .bind(status)
    .fetch_optional(pool)
    .await
}
