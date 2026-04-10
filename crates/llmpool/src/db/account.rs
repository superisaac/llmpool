use crate::db::DbPool;
use crate::models::{Account, NewAccount, UpdateAccount};

/// Create a new account
pub async fn create_account(
    pool: &DbPool,
    new_account: &NewAccount,
) -> Result<Account, sqlx::Error> {
    sqlx::query_as::<_, Account>("INSERT INTO accounts (name) VALUES ($1) RETURNING *")
        .bind(&new_account.name)
        .fetch_one(pool)
        .await
}

/// Get an account by ID
pub async fn get_account_by_id(
    pool: &DbPool,
    account_id: i64,
) -> Result<Option<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
}

/// Get an account by name
pub async fn get_account_by_name(
    pool: &DbPool,
    name: &str,
) -> Result<Option<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await
}

/// Count total number of accounts
pub async fn count_accounts(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List accounts with pagination.
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_accounts_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts ORDER BY id ASC LIMIT $1 OFFSET $2")
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
}

/// Update an account by ID. Only the provided fields will be updated.
pub async fn update_account(
    pool: &DbPool,
    account_id: i64,
    update: &UpdateAccount,
) -> Result<Account, sqlx::Error> {
    sqlx::query_as::<_, Account>(
        "UPDATE accounts SET
            name = COALESCE($2, name),
            is_active = COALESCE($3, is_active),
            updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(account_id)
    .bind(&update.name)
    .bind(update.is_active)
    .fetch_one(pool)
    .await
}
