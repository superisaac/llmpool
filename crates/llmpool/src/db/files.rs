use crate::db::DbPool;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FileMeta {
    pub id: i64,
    pub file_id: String,
    pub original_file_id: String,
    pub purpose: String,
    pub upstream_id: i32,
    pub deleted: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// ── DB operations ─────────────────────────────────────────────────────────────

/// Insert a new FileMeta record.
pub async fn create_file_meta(
    pool: &DbPool,
    file_id: &str,
    original_file_id: &str,
    purpose: &str,
    upstream_id: i32,
) -> Result<FileMeta, sqlx::Error> {
    sqlx::query_as::<_, FileMeta>(
        "INSERT INTO file_metas (file_id, original_file_id, purpose, upstream_id)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(file_id)
    .bind(original_file_id)
    .bind(purpose)
    .bind(upstream_id)
    .fetch_one(pool)
    .await
}

/// Look up a FileMeta by our internal file_id.
pub async fn get_file_meta_by_file_id(
    pool: &DbPool,
    file_id: &str,
) -> Result<Option<FileMeta>, sqlx::Error> {
    sqlx::query_as::<_, FileMeta>("SELECT * FROM file_metas WHERE file_id = $1 AND deleted = FALSE")
        .bind(file_id)
        .fetch_optional(pool)
        .await
}

/// Mark a FileMeta as deleted by our internal file_id.
pub async fn mark_file_meta_deleted(
    pool: &DbPool,
    file_id: &str,
) -> Result<Option<FileMeta>, sqlx::Error> {
    sqlx::query_as::<_, FileMeta>(
        "UPDATE file_metas
         SET deleted = TRUE, updated_at = NOW()
         WHERE file_id = $1
         RETURNING *",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
}
