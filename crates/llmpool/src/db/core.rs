use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::config;

pub type DbPool = PgPool;

/// Create a connection pool from a database URL
pub async fn create_pool(database_url: &str) -> DbPool {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .expect("Failed to create database connection pool")
}

/// Create a connection pool using the database URL.
///
/// Priority:
/// 1. `DB_URL` environment variable (if set)
/// 2. Config file `[database] url` value
pub async fn create_pool_from_config() -> DbPool {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let cfg = config::get_config();
        cfg.database.url.clone()
    });
    create_pool(&database_url).await
}

/// Run database migrations
pub async fn run_migrations(pool: &DbPool) {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .expect("Failed to run database migrations");
}
