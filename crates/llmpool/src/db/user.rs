use crate::db::DbPool;
use crate::models::{NewUser, User};

/// Create a new user
pub async fn create_user(pool: &DbPool, new_user: &NewUser) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>("INSERT INTO users (username) VALUES ($1) RETURNING *")
        .bind(&new_user.username)
        .fetch_one(pool)
        .await
}
