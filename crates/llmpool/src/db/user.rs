use crate::db::DbPool;
use crate::models::{NewUser, UpdateUser, User};

/// Create a new user
pub async fn create_user(pool: &DbPool, new_user: &NewUser) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>("INSERT INTO users (username) VALUES ($1) RETURNING *")
        .bind(&new_user.username)
        .fetch_one(pool)
        .await
}

/// Get a user by ID
pub async fn get_user_by_id(pool: &DbPool, user_id: i32) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Get a user by username
pub async fn get_user_by_username(
    pool: &DbPool,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
}

/// Count total number of users
pub async fn count_users(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List users with pagination.
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_users_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY id ASC LIMIT $1 OFFSET $2")
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
}

/// Update a user by ID. Only the provided fields will be updated.
pub async fn update_user(
    pool: &DbPool,
    user_id: i32,
    update: &UpdateUser,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "UPDATE users SET
            username = COALESCE($2, username),
            is_active = COALESCE($3, is_active),
            updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(user_id)
    .bind(&update.username)
    .bind(update.is_active)
    .fetch_one(pool)
    .await
}
