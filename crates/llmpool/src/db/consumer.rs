use crate::db::DbPool;
use crate::models::{Consumer, NewConsumer, UpdateConsumer};

/// Create a new consumer
pub async fn create_consumer(
    pool: &DbPool,
    new_consumer: &NewConsumer,
) -> Result<Consumer, sqlx::Error> {
    sqlx::query_as::<_, Consumer>("INSERT INTO consumers (name) VALUES ($1) RETURNING *")
        .bind(&new_consumer.name)
        .fetch_one(pool)
        .await
}

/// Get a consumer by ID
pub async fn get_consumer_by_id(
    pool: &DbPool,
    consumer_id: i32,
) -> Result<Option<Consumer>, sqlx::Error> {
    sqlx::query_as::<_, Consumer>("SELECT * FROM consumers WHERE id = $1")
        .bind(consumer_id)
        .fetch_optional(pool)
        .await
}

/// Get a consumer by name
pub async fn get_consumer_by_name(
    pool: &DbPool,
    name: &str,
) -> Result<Option<Consumer>, sqlx::Error> {
    sqlx::query_as::<_, Consumer>("SELECT * FROM consumers WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await
}

/// Count total number of consumers
pub async fn count_consumers(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM consumers")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List consumers with pagination.
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_consumers_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<Consumer>, sqlx::Error> {
    sqlx::query_as::<_, Consumer>("SELECT * FROM consumers ORDER BY id ASC LIMIT $1 OFFSET $2")
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
}

/// Update a consumer by ID. Only the provided fields will be updated.
pub async fn update_consumer(
    pool: &DbPool,
    consumer_id: i32,
    update: &UpdateConsumer,
) -> Result<Consumer, sqlx::Error> {
    sqlx::query_as::<_, Consumer>(
        "UPDATE consumers SET
            name = COALESCE($2, name),
            is_active = COALESCE($3, is_active),
            updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(consumer_id)
    .bind(&update.name)
    .bind(update.is_active)
    .fetch_one(pool)
    .await
}
