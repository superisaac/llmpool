use uuid::Uuid;

use crate::db::DbPool;
use crate::models::{AccessKey, NewAccessKey, User};

/// Find an access key by its apikey string (only active keys).
pub async fn find_active_access_key_by_apikey(
    pool: &DbPool,
    apikey: &str,
) -> Result<Option<AccessKey>, sqlx::Error> {
    sqlx::query_as::<_, AccessKey>(
        "SELECT * FROM access_keys WHERE apikey = $1 AND is_active = true",
    )
    .bind(apikey)
    .fetch_optional(pool)
    .await
}

/// Find a user by their ID.
pub async fn find_user_by_id(pool: &DbPool, user_id: i32) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Generate a random access key string with the prefix "lpx-"
/// Uses UUIDv7 algorithm (time-ordered with random bits) and outputs as hex string
fn generate_access_key() -> String {
    let uuid = Uuid::now_v7();
    let hex_string = uuid.simple().to_string();
    format!("lpx-{}", hex_string)
}

/// Find a user by username
pub async fn find_user_by_username(
    pool: &DbPool,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
}

/// Create a new access key for a user
pub async fn create_access_key_for_user(
    pool: &DbPool,
    user_id: i32,
) -> Result<AccessKey, sqlx::Error> {
    let apikey = generate_access_key();
    let new_key = NewAccessKey {
        user_id: Some(user_id),
        apikey,
        expires_at: None,
    };
    sqlx::query_as::<_, AccessKey>(
        "INSERT INTO access_keys (user_id, apikey, expires_at)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(new_key.user_id)
    .bind(&new_key.apikey)
    .bind(new_key.expires_at)
    .fetch_one(pool)
    .await
}
