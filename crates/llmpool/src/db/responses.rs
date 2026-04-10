use crate::db::DbPool;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ResponseMeta {
    pub id: i64,
    pub response_id: String,
    pub original_response_id: String,
    pub upstream_id: i64,
    pub deleted: bool,
    pub created_at: NaiveDateTime,
}

// ── DB operations ─────────────────────────────────────────────────────────────

/// Insert a new ResponseMeta record.
pub async fn create_response_meta(
    pool: &DbPool,
    response_id: &str,
    original_response_id: &str,
    upstream_id: i64,
) -> Result<ResponseMeta, sqlx::Error> {
    sqlx::query_as::<_, ResponseMeta>(
        "INSERT INTO response_metas (response_id, original_response_id, upstream_id)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(response_id)
    .bind(original_response_id)
    .bind(upstream_id)
    .fetch_one(pool)
    .await
}

/// Look up a ResponseMeta by our internal response_id.
pub async fn get_response_meta_by_response_id(
    pool: &DbPool,
    response_id: &str,
) -> Result<Option<ResponseMeta>, sqlx::Error> {
    sqlx::query_as::<_, ResponseMeta>(
        "SELECT * FROM response_metas WHERE response_id = $1 AND deleted = FALSE",
    )
    .bind(response_id)
    .fetch_optional(pool)
    .await
}

/// Look up a ResponseMeta by the upstream's original_response_id.
pub async fn get_response_meta_by_original_response_id(
    pool: &DbPool,
    original_response_id: &str,
) -> Result<Option<ResponseMeta>, sqlx::Error> {
    sqlx::query_as::<_, ResponseMeta>(
        "SELECT * FROM response_metas WHERE original_response_id = $1 AND deleted = FALSE",
    )
    .bind(original_response_id)
    .fetch_optional(pool)
    .await
}

/// Given our internal response_id, return the upstream's original_response_id.
/// Returns None if no mapping exists.
pub async fn get_original_response_id(
    pool: &DbPool,
    response_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT original_response_id FROM response_metas WHERE response_id = $1 AND deleted = FALSE",
    )
    .bind(response_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v))
}

/// Given the upstream's original_response_id, return our internal response_id.
/// Returns None if no mapping exists.
pub async fn get_response_id_from_original_response_id(
    pool: &DbPool,
    original_response_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT response_id FROM response_metas WHERE original_response_id = $1 AND deleted = FALSE",
    )
    .bind(original_response_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v))
}

/// Mark a ResponseMeta as deleted by our internal response_id.
pub async fn mark_response_meta_deleted(
    pool: &DbPool,
    response_id: &str,
) -> Result<Option<ResponseMeta>, sqlx::Error> {
    sqlx::query_as::<_, ResponseMeta>(
        "UPDATE response_metas
         SET deleted = TRUE
         WHERE response_id = $1
         RETURNING *",
    )
    .bind(response_id)
    .fetch_optional(pool)
    .await
}
