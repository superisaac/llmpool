use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{Account, NewOpenAIAPIKey, OpenAIAPIKey};

/// Find an API key by its apikey string (only active keys).
pub async fn find_active_api_key_by_apikey(
    pool: &DbPool,
    apikey: &str,
) -> Result<Option<OpenAIAPIKey>, sqlx::Error> {
    sqlx::query_as::<_, OpenAIAPIKey>(
        "SELECT * FROM llm_api_keys WHERE apikey = $1 AND is_active = true",
    )
    .bind(apikey)
    .fetch_optional(pool)
    .await
}

/// Find an account by their ID.
pub async fn find_account_by_id(
    pool: &DbPool,
    account_id: i32,
) -> Result<Option<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
}

/// Generate a random API key string with the prefix "lpx-"
/// Uses UUIDv7 algorithm (time-ordered with random bits) and outputs as hex string
fn generate_api_key() -> String {
    let uuid = Uuid::now_v7();
    let hex_string = uuid.simple().to_string();
    format!("lpx-{}", hex_string)
}

/// Find an account by name
pub async fn find_account_by_name(
    pool: &DbPool,
    name: &str,
) -> Result<Option<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await
}

/// Count total number of API keys for a consumer
pub async fn count_api_keys_by_consumer(
    pool: &DbPool,
    account_id: i32,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_api_keys WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List API keys for a consumer with pagination.
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_api_keys_by_account_paginated(
    pool: &DbPool,
    account_id: i32,
    offset: i64,
    limit: i64,
) -> Result<Vec<OpenAIAPIKey>, sqlx::Error> {
    sqlx::query_as::<_, OpenAIAPIKey>(
        "SELECT * FROM llm_api_keys WHERE account_id = $1 ORDER BY id ASC LIMIT $2 OFFSET $3",
    )
    .bind(account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Create a new API key for a consumer
pub async fn create_api_key_for_consumer(
    pool: &DbPool,
    account_id: i32,
    label: &str,
) -> Result<OpenAIAPIKey, sqlx::Error> {
    let apikey = generate_api_key();
    let new_key = NewOpenAIAPIKey {
        account_id: Some(account_id),
        apikey,
        label: label.to_string(),
        expires_at: None,
    };
    sqlx::query_as::<_, OpenAIAPIKey>(
        "INSERT INTO llm_api_keys (account_id, apikey, label, expires_at)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(new_key.account_id)
    .bind(&new_key.apikey)
    .bind(&new_key.label)
    .bind(new_key.expires_at)
    .fetch_one(pool)
    .await
}
