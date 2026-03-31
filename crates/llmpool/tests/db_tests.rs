//! Integration tests for database operations.
//!
//! These tests require a running PostgreSQL instance. Set the `DATABASE_URL`
//! environment variable before running:
//!
//! ```bash
//! DATABASE_URL="postgres://user:pass@localhost/llmpool_test" cargo test --test db_tests
//! ```
//!
//! The test database should have migrations applied. Each test uses a transaction
//! that is rolled back at the end, so the database state is not affected.

use bigdecimal::BigDecimal;
use sqlx::PgPool;
use std::str::FromStr;

// Re-use the library's modules
use llmpool::models::*;

/// Helper: create a test pool from DATABASE_URL env var
async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration tests");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Helper: create a test account within a transaction and return it
async fn create_test_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
) -> Account {
    sqlx::query_as::<_, Account>("INSERT INTO accounts (name) VALUES ($1) RETURNING *")
        .bind(name)
        .fetch_one(&mut **tx)
        .await
        .expect("Failed to create test account")
}

/// Helper: create an upstream within a transaction (no encryption)
async fn create_test_upstream(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
    api_base: &str,
) -> LLMUpstream {
    sqlx::query_as::<_, LLMUpstream>(
        "INSERT INTO llm_upstreams (name, api_base, api_key, has_responses_api, tags, proxies, status, description)
         VALUES ($1, $2, 'test-key', false, '{}', '{}', 'online', '')
         RETURNING *",
    )
    .bind(name)
    .bind(api_base)
    .fetch_one(&mut **tx)
    .await
    .expect("Failed to create test upstream")
}

/// Helper: create a model within a transaction
async fn create_test_model(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    upstream_id: i32,
    model_id: &str,
) -> LLMModel {
    sqlx::query_as::<_, LLMModel>(
        "INSERT INTO llm_models (upstream_id, model_id, has_chat_completion, has_embedding, has_image_generation, has_speech, input_token_price, output_token_price)
         VALUES ($1, $2, true, false, false, false, 0.000001, 0.000001)
         RETURNING *",
    )
    .bind(upstream_id)
    .bind(model_id)
    .fetch_one(&mut **tx)
    .await
    .expect("Failed to create test model")
}

// ============================================================
// Account DB Tests
// ============================================================

mod account_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_account() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let new_account = NewAccount {
            name: "test_account_create".to_string(),
        };

        let account =
            sqlx::query_as::<_, Account>("INSERT INTO accounts (name) VALUES ($1) RETURNING *")
                .bind(&new_account.name)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(account.name, "test_account_create");
        assert!(account.is_active);
        assert!(account.id > 0);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_account_duplicate_name() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let name = "test_account_dup";
        create_test_account(&mut tx, name).await;

        // Attempt to create another account with the same name should fail (unique index)
        let result =
            sqlx::query_as::<_, Account>("INSERT INTO accounts (name) VALUES ($1) RETURNING *")
                .bind(name)
                .fetch_one(&mut *tx)
                .await;

        assert!(result.is_err(), "Duplicate account name should fail");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_account_by_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_get_by_id").await;

        let found = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = $1")
            .bind(account.id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test_get_by_id");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_account_by_id_not_found() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let found = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = $1")
            .bind(999999)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_account_by_name() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        create_test_account(&mut tx, "test_get_by_name").await;

        let found = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE name = $1")
            .bind("test_get_by_name")
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test_get_by_name");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_account_by_name_not_found() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let found = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE name = $1")
            .bind("nonexistent_account")
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_count_accounts() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        create_test_account(&mut tx, "test_count_1").await;
        create_test_account(&mut tx, "test_count_2").await;

        let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(after.0, before.0 + 2);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_accounts_paginated() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Create several accounts
        for i in 0..5 {
            create_test_account(&mut tx, &format!("test_paginated_{}", i)).await;
        }

        // Fetch page 1 (limit 2, offset 0)
        let page1 = sqlx::query_as::<_, Account>(
            "SELECT * FROM accounts ORDER BY id ASC LIMIT $1 OFFSET $2",
        )
        .bind(2i64)
        .bind(0i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(page1.len(), 2);

        // Fetch page 2 (limit 2, offset 2)
        let page2 = sqlx::query_as::<_, Account>(
            "SELECT * FROM accounts ORDER BY id ASC LIMIT $1 OFFSET $2",
        )
        .bind(2i64)
        .bind(2i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(page2.len(), 2);

        // Ensure pages don't overlap
        assert_ne!(page1[0].id, page2[0].id);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_account_name() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_update_name_old").await;

        let updated = sqlx::query_as::<_, Account>(
            "UPDATE accounts SET name = COALESCE($2, name), is_active = COALESCE($3, is_active), updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(account.id)
        .bind(Some("test_update_name_new"))
        .bind(None::<bool>)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.name, "test_update_name_new");
        assert!(updated.is_active); // unchanged

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_account_deactivate() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_deactivate").await;
        assert!(account.is_active);

        let updated = sqlx::query_as::<_, Account>(
            "UPDATE accounts SET name = COALESCE($2, name), is_active = COALESCE($3, is_active), updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(account.id)
        .bind(None::<String>)
        .bind(Some(false))
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert!(!updated.is_active);
        assert_eq!(updated.name, "test_deactivate"); // unchanged

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_account_both_fields() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_update_both").await;

        let updated = sqlx::query_as::<_, Account>(
            "UPDATE accounts SET name = COALESCE($2, name), is_active = COALESCE($3, is_active), updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(account.id)
        .bind(Some("test_update_both_new"))
        .bind(Some(false))
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.name, "test_update_both_new");
        assert!(!updated.is_active);

        tx.rollback().await.unwrap();
    }
}

// ============================================================
// API Key DB Tests
// ============================================================

mod api_key_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_api_key() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_apikey_account").await;

        let api_key = sqlx::query_as::<_, ApiCredential>(
            "INSERT INTO api_credentials (account_id, apikey, label, expires_at) VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(account.id)
        .bind("lpx-testapikey123")
        .bind("test label")
        .bind(None::<chrono::NaiveDateTime>)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(api_key.account_id, Some(account.id));
        assert_eq!(api_key.apikey, "lpx-testapikey123");
        assert_eq!(api_key.label, "test label");
        assert!(api_key.is_active);
        assert!(api_key.expires_at.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_api_key_duplicate_apikey() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_apikey_dup_account").await;

        // Create first key
        sqlx::query("INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3)")
            .bind(account.id)
            .bind("lpx-duplicate-key")
            .bind("first")
            .execute(&mut *tx)
            .await
            .unwrap();

        // Attempt duplicate
        let result = sqlx::query(
            "INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3)",
        )
        .bind(account.id)
        .bind("lpx-duplicate-key")
        .bind("second")
        .execute(&mut *tx)
        .await;

        assert!(result.is_err(), "Duplicate apikey should fail");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_active_api_key() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_find_active_key").await;

        sqlx::query(
            "INSERT INTO api_credentials (account_id, apikey, label, is_active) VALUES ($1, $2, $3, $4)",
        )
        .bind(account.id)
        .bind("lpx-active-key")
        .bind("active")
        .bind(true)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Find active key
        let found = sqlx::query_as::<_, ApiCredential>(
            "SELECT * FROM api_credentials WHERE apikey = $1 AND is_active = true",
        )
        .bind("lpx-active-key")
        .fetch_optional(&mut *tx)
        .await
        .unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().apikey, "lpx-active-key");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_inactive_api_key_returns_none() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_find_inactive_key").await;

        sqlx::query(
            "INSERT INTO api_credentials (account_id, apikey, label, is_active) VALUES ($1, $2, $3, $4)",
        )
        .bind(account.id)
        .bind("lpx-inactive-key")
        .bind("inactive")
        .bind(false)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Should not find inactive key
        let found = sqlx::query_as::<_, ApiCredential>(
            "SELECT * FROM api_credentials WHERE apikey = $1 AND is_active = true",
        )
        .bind("lpx-inactive-key")
        .fetch_optional(&mut *tx)
        .await
        .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_count_api_keys_by_account() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_count_keys").await;

        // Create 3 keys
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3)",
            )
            .bind(account.id)
            .bind(format!("lpx-count-key-{}", i))
            .bind(format!("key {}", i))
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM api_credentials WHERE account_id = $1")
                .bind(account.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(count.0, 3);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_api_keys_paginated() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_list_keys_paged").await;

        // Create 5 keys
        for i in 0..5 {
            sqlx::query(
                "INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3)",
            )
            .bind(account.id)
            .bind(format!("lpx-paged-key-{}", i))
            .bind(format!("key {}", i))
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        // Page 1
        let page1 = sqlx::query_as::<_, ApiCredential>(
            "SELECT * FROM api_credentials WHERE account_id = $1 ORDER BY id ASC LIMIT $2 OFFSET $3",
        )
        .bind(account.id)
        .bind(2i64)
        .bind(0i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(page1.len(), 2);

        // Page 3 (should have 1 item)
        let page3 = sqlx::query_as::<_, ApiCredential>(
            "SELECT * FROM api_credentials WHERE account_id = $1 ORDER BY id ASC LIMIT $2 OFFSET $3",
        )
        .bind(account.id)
        .bind(2i64)
        .bind(4i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(page3.len(), 1);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_api_key_cascade_delete_on_account() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "test_cascade_delete").await;

        sqlx::query("INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3)")
            .bind(account.id)
            .bind("lpx-cascade-key")
            .bind("cascade")
            .execute(&mut *tx)
            .await
            .unwrap();

        // Delete account
        sqlx::query("DELETE FROM accounts WHERE id = $1")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // API key should be gone (CASCADE)
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM api_credentials WHERE account_id = $1")
                .bind(account.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(count.0, 0);

        tx.rollback().await.unwrap();
    }
}

// ============================================================
// OpenAI Upstream & Model DB Tests
// ============================================================

mod openai_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_upstream() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream = create_test_upstream(&mut tx, "test-ep", "https://api.test.com/v1").await;

        assert_eq!(upstream.name, "test-ep");
        assert_eq!(upstream.api_base, "https://api.test.com/v1");
        assert_eq!(upstream.api_key, "test-key");
        assert!(!upstream.has_responses_api);
        assert_eq!(upstream.status, "online");
        assert!(upstream.tags.is_empty());
        assert!(upstream.proxies.is_empty());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_upstream_duplicate_api_base() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        create_test_upstream(&mut tx, "ep1", "https://api.dup.com/v1").await;

        let result = sqlx::query_as::<_, LLMUpstream>(
            "INSERT INTO llm_upstreams (name, api_base, api_key) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind("ep2")
        .bind("https://api.dup.com/v1")
        .bind("key2")
        .fetch_one(&mut *tx)
        .await;

        assert!(result.is_err(), "Duplicate api_base should fail");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_upstream_by_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "test-get-ep", "https://api.getep.com/v1").await;

        let found = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE id = $1")
            .bind(upstream.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(found.name, "test-get-ep");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_upstream_by_name() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        create_test_upstream(&mut tx, "test-name-ep", "https://api.nameep.com/v1").await;

        let found = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE name = $1")
            .bind("test-name-ep")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(found.api_base, "https://api.nameep.com/v1");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_upstream() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "test-update-ep", "https://api.updateep.com/v1").await;

        let updated = sqlx::query_as::<_, LLMUpstream>(
            "UPDATE llm_upstreams SET name = $1, status = $2, updated_at = NOW() WHERE id = $3 RETURNING *",
        )
        .bind("updated-ep-name")
        .bind("offline")
        .bind(upstream.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.name, "updated-ep-name");
        assert_eq!(updated.status, "offline");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_upstream() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "test-delete-ep", "https://api.deleteep.com/v1").await;

        let result = sqlx::query("DELETE FROM llm_upstreams WHERE id = $1")
            .bind(upstream.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        assert_eq!(result.rows_affected(), 1);

        // Verify it's gone
        let found = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE id = $1")
            .bind(upstream.id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_count_upstreams() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_upstreams")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        create_test_upstream(&mut tx, "count-ep-1", "https://api.count1.com/v1").await;
        create_test_upstream(&mut tx, "count-ep-2", "https://api.count2.com/v1").await;

        let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_upstreams")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(after.0, before.0 + 2);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_upstream_tags() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream = create_test_upstream(&mut tx, "tag-ep", "https://api.tagep.com/v1").await;

        // Add a tag
        let updated = sqlx::query_as::<_, LLMUpstream>(
            "UPDATE llm_upstreams SET tags = CASE WHEN $2 = ANY(tags) THEN tags ELSE array_append(tags, $2) END, updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(upstream.id)
        .bind("production")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.tags, vec!["production"]);

        // Add another tag
        let updated = sqlx::query_as::<_, LLMUpstream>(
            "UPDATE llm_upstreams SET tags = CASE WHEN $2 = ANY(tags) THEN tags ELSE array_append(tags, $2) END, updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(upstream.id)
        .bind("fast")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.tags, vec!["production", "fast"]);

        // Adding duplicate tag should not change
        let updated = sqlx::query_as::<_, LLMUpstream>(
            "UPDATE llm_upstreams SET tags = CASE WHEN $2 = ANY(tags) THEN tags ELSE array_append(tags, $2) END, updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(upstream.id)
        .bind("production")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.tags, vec!["production", "fast"]);

        // Remove a tag
        let updated = sqlx::query_as::<_, LLMUpstream>(
            "UPDATE llm_upstreams SET tags = array_remove(tags, $2), updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(upstream.id)
        .bind("production")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.tags, vec!["fast"]);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_upstreams_by_tag() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Create upstreams with different tags
        let ep1 =
            create_test_upstream(&mut tx, "tag-search-1", "https://api.tagsearch1.com/v1").await;
        let _ep2 =
            create_test_upstream(&mut tx, "tag-search-2", "https://api.tagsearch2.com/v1").await;

        // Add tag to ep1 only
        sqlx::query("UPDATE llm_upstreams SET tags = array_append(tags, $2) WHERE id = $1")
            .bind(ep1.id)
            .bind("special")
            .execute(&mut *tx)
            .await
            .unwrap();

        let found =
            sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE $1 = ANY(tags)")
                .bind("special")
                .fetch_all(&mut *tx)
                .await
                .unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "tag-search-1");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_model() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "model-ep", "https://api.modelep.com/v1").await;
        let model = create_test_model(&mut tx, upstream.id, "gpt-4").await;

        assert_eq!(model.upstream_id, upstream.id);
        assert_eq!(model.model_id, "gpt-4");
        assert!(model.has_chat_completion);
        assert!(!model.has_embedding);
        assert!(!model.has_image_generation);
        assert!(!model.has_speech);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_model_duplicate() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "dup-model-ep", "https://api.dupmodel.com/v1").await;
        create_test_model(&mut tx, upstream.id, "gpt-4").await;

        // Duplicate (upstream_id, model_id) should fail
        let result = sqlx::query_as::<_, LLMModel>(
            "INSERT INTO llm_models (upstream_id, model_id) VALUES ($1, $2) RETURNING *",
        )
        .bind(upstream.id)
        .bind("gpt-4")
        .fetch_one(&mut *tx)
        .await;

        assert!(
            result.is_err(),
            "Duplicate (upstream_id, model_id) should fail"
        );

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_models_by_upstream() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "list-model-ep", "https://api.listmodel.com/v1").await;
        create_test_model(&mut tx, upstream.id, "gpt-4").await;
        create_test_model(&mut tx, upstream.id, "gpt-3.5-turbo").await;

        let models =
            sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE upstream_id = $1")
                .bind(upstream.id)
                .fetch_all(&mut *tx)
                .await
                .unwrap();

        assert_eq!(models.len(), 2);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_model_by_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "get-model-ep", "https://api.getmodel.com/v1").await;
        let model = create_test_model(&mut tx, upstream.id, "gpt-4").await;

        let found = sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE id = $1")
            .bind(model.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(found.model_id, "gpt-4");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_model_by_upstream_and_model_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "find-model-ep", "https://api.findmodel.com/v1").await;
        create_test_model(&mut tx, upstream.id, "claude-3").await;

        let found = sqlx::query_as::<_, LLMModel>(
            "SELECT * FROM llm_models WHERE upstream_id = $1 AND model_id = $2",
        )
        .bind(upstream.id)
        .bind("claude-3")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(found.model_id, "claude-3");
        assert_eq!(found.upstream_id, upstream.id);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_model() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "upd-model-ep", "https://api.updmodel.com/v1").await;
        let model = create_test_model(&mut tx, upstream.id, "gpt-4").await;

        let updated = sqlx::query_as::<_, LLMModel>(
            "UPDATE llm_models SET has_embedding = true, has_image_generation = true, updated_at = NOW() WHERE id = $1 RETURNING *",
        )
        .bind(model.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert!(updated.has_embedding);
        assert!(updated.has_image_generation);
        assert!(updated.has_chat_completion); // unchanged

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_model() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "del-model-ep", "https://api.delmodel.com/v1").await;
        let model = create_test_model(&mut tx, upstream.id, "gpt-4").await;

        let result = sqlx::query("DELETE FROM llm_models WHERE id = $1")
            .bind(model.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        assert_eq!(result.rows_affected(), 1);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_models_cascade_delete_on_upstream() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream = create_test_upstream(
            &mut tx,
            "cascade-model-ep",
            "https://api.cascademodel.com/v1",
        )
        .await;
        create_test_model(&mut tx, upstream.id, "gpt-4").await;
        create_test_model(&mut tx, upstream.id, "gpt-3.5").await;

        // Delete upstream
        sqlx::query("DELETE FROM llm_upstreams WHERE id = $1")
            .bind(upstream.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Models should be gone (CASCADE)
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM llm_models WHERE upstream_id = $1")
                .bind(upstream.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(count.0, 0);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_first_model_by_name() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let ep1 =
            create_test_upstream(&mut tx, "first-model-ep1", "https://api.firstmodel1.com/v1")
                .await;
        let ep2 =
            create_test_upstream(&mut tx, "first-model-ep2", "https://api.firstmodel2.com/v1")
                .await;

        create_test_model(&mut tx, ep1.id, "shared-model").await;
        create_test_model(&mut tx, ep2.id, "shared-model").await;

        let found =
            sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE model_id = $1 LIMIT 1")
                .bind("shared-model")
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(found.model_id, "shared-model");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_upstreams_paginated() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        for i in 0..5 {
            create_test_upstream(
                &mut tx,
                &format!("paged-ep-{}", i),
                &format!("https://api.paged{}.com/v1", i),
            )
            .await;
        }

        let page = sqlx::query_as::<_, LLMUpstream>(
            "SELECT * FROM llm_upstreams ORDER BY id ASC LIMIT $1 OFFSET $2",
        )
        .bind(3i64)
        .bind(0i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert!(page.len() <= 3);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_model_token_prices() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let upstream =
            create_test_upstream(&mut tx, "price-ep", "https://api.priceep.com/v1").await;

        let model = sqlx::query_as::<_, LLMModel>(
            "INSERT INTO llm_models (upstream_id, model_id, input_token_price, output_token_price)
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(upstream.id)
        .bind("expensive-model")
        .bind(BigDecimal::from_str("0.03").unwrap())
        .bind(BigDecimal::from_str("0.06").unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(
            model.input_token_price,
            BigDecimal::from_str("0.03").unwrap()
        );
        assert_eq!(
            model.output_token_price,
            BigDecimal::from_str("0.06").unwrap()
        );

        tx.rollback().await.unwrap();
    }
}

// ============================================================
// Fund DB Tests
// ============================================================

mod fund_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_fund() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "fund_account").await;

        let fund = sqlx::query_as::<_, Fund>(
            "INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(account.id)
        .bind(BigDecimal::from(100))
        .bind(BigDecimal::from(50))
        .bind(BigDecimal::from(0))
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(fund.account_id, account.id);
        assert_eq!(fund.cash, BigDecimal::from(100));
        assert_eq!(fund.credit, BigDecimal::from(50));
        assert_eq!(fund.debt, BigDecimal::from(0));

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_fund_duplicate_account() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "fund_dup_account").await;

        sqlx::query("INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 0, 0, 0)")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Duplicate account_id should fail (unique index)
        let result =
            sqlx::query("INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 0, 0, 0)")
                .bind(account.id)
                .execute(&mut *tx)
                .await;

        assert!(result.is_err(), "Duplicate account_id in funds should fail");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_account_fund() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "find_fund_account").await;

        sqlx::query("INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 200, 100, 0)")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let found = sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
            .bind(account.id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_some());
        let fund = found.unwrap();
        assert_eq!(fund.cash, BigDecimal::from(200));
        assert_eq!(fund.credit, BigDecimal::from(100));

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_account_fund_not_found() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let found = sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
            .bind(999999)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_update_fund() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "update_fund_account").await;

        let fund = sqlx::query_as::<_, Fund>(
            "INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 100, 50, 0) RETURNING *",
        )
        .bind(account.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        let updated = sqlx::query_as::<_, Fund>(
            "UPDATE funds SET cash = $1, credit = $2, debt = $3, updated_at = NOW() WHERE id = $4 RETURNING *",
        )
        .bind(BigDecimal::from(80))
        .bind(BigDecimal::from(30))
        .bind(BigDecimal::from(10))
        .bind(fund.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(updated.cash, BigDecimal::from(80));
        assert_eq!(updated.credit, BigDecimal::from(30));
        assert_eq!(updated.debt, BigDecimal::from(10));

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_fund_available() {
        let fund = Fund {
            id: 1,
            account_id: 1,
            cash: BigDecimal::from(100),
            credit: BigDecimal::from(50),
            debt: BigDecimal::from(20),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };

        // available = cash + credit
        assert_eq!(fund.available(), BigDecimal::from(150));
    }

    #[tokio::test]
    async fn test_fund_available_zero() {
        let fund = Fund {
            id: 1,
            account_id: 1,
            cash: BigDecimal::from(0),
            credit: BigDecimal::from(0),
            debt: BigDecimal::from(100),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };

        assert_eq!(fund.available(), BigDecimal::from(0));
    }
}

// ============================================================
// Session Event DB Tests
// ============================================================

mod session_event_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session_event() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({
            "type": "chat_completion",
            "model": "gpt-4",
            "tokens": 100
        });

        let event = sqlx::query_as::<_, SessionEvent>(
            "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind("sess-001")
        .bind(0)
        .bind(1)
        .bind(1)
        .bind(1)
        .bind(&event_data)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(event.session_id, "sess-001");
        assert_eq!(event.session_index, 0);
        assert_eq!(event.account_id, 1);
        assert_eq!(event.event_data["type"], "chat_completion");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_session_event_upsert() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data1 = serde_json::json!({"version": 1});
        let event_data2 = serde_json::json!({"version": 2});

        // Insert first
        sqlx::query(
            "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (session_id, session_index) DO UPDATE SET event_data = EXCLUDED.event_data",
        )
        .bind("sess-upsert")
        .bind(0)
        .bind(1)
        .bind(1)
        .bind(1)
        .bind(&event_data1)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Upsert with same (session_id, session_index)
        let event = sqlx::query_as::<_, SessionEvent>(
            "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (session_id, session_index) DO UPDATE SET event_data = EXCLUDED.event_data
             RETURNING *",
        )
        .bind("sess-upsert")
        .bind(0)
        .bind(1)
        .bind(1)
        .bind(1)
        .bind(&event_data2)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(event.event_data["version"], 2);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_session_events_all() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({"test": true});

        for i in 0..3 {
            sqlx::query(
                "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(format!("sess-list-{}", i))
            .bind(0)
            .bind(1)
            .bind(1)
            .bind(1)
            .bind(&event_data)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        let events = sqlx::query_as::<_, SessionEvent>(
            "SELECT * FROM session_events ORDER BY id DESC LIMIT $1 OFFSET $2",
        )
        .bind(10i64)
        .bind(0i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert!(events.len() >= 3);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_list_session_events_by_session_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({"test": true});

        // Create events for two different sessions
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind("sess-filter-target")
            .bind(i)
            .bind(1)
            .bind(1)
            .bind(1)
            .bind(&event_data)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        sqlx::query(
            "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind("sess-filter-other")
        .bind(0)
        .bind(1)
        .bind(1)
        .bind(1)
        .bind(&event_data)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Filter by session_id
        let events = sqlx::query_as::<_, SessionEvent>(
            "SELECT * FROM session_events WHERE session_id = $1 ORDER BY id ASC LIMIT $2 OFFSET $3",
        )
        .bind("sess-filter-target")
        .bind(10i64)
        .bind(0i64)
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(events.len(), 3);
        for event in &events {
            assert_eq!(event.session_id, "sess-filter-target");
        }

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_count_session_events() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({"test": true});

        let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM session_events")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        for i in 0..3 {
            sqlx::query(
                "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(format!("sess-count-{}", i))
            .bind(0)
            .bind(1)
            .bind(1)
            .bind(1)
            .bind(&event_data)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM session_events")
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(after.0, before.0 + 3);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_count_session_events_by_session_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({"test": true});

        for i in 0..2 {
            sqlx::query(
                "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind("sess-count-filter")
            .bind(i)
            .bind(1)
            .bind(1)
            .bind(1)
            .bind(&event_data)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM session_events WHERE session_id = $1")
                .bind("sess-count-filter")
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert_eq!(count.0, 2);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_session_event_multiple_indices() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let event_data = serde_json::json!({"test": true});

        // Create events with different session_index values
        for i in 0..5 {
            sqlx::query(
                "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_key_id, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind("sess-multi-idx")
            .bind(i)
            .bind(1)
            .bind(1)
            .bind(1)
            .bind(&event_data)
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        let events = sqlx::query_as::<_, SessionEvent>(
            "SELECT * FROM session_events WHERE session_id = $1 ORDER BY session_index ASC",
        )
        .bind("sess-multi-idx")
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        assert_eq!(events.len(), 5);
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.session_index, i as i32);
        }

        tx.rollback().await.unwrap();
    }
}

// ============================================================
// Balance Change DB Tests
// ============================================================

mod balance_change_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_balance_change() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_account").await;

        let content = serde_json::json!({
            "type": "Deposit",
            "amount": "100.00"
        });

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-001")
        .bind(&content)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(bc.account_id, account.id);
        assert_eq!(bc.unique_request_id, "req-001");
        assert!(!bc.is_applied);
        assert_eq!(bc.content["type"], "Deposit");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_balance_change_duplicate_request_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_dup_account").await;

        let content = serde_json::json!({"type": "Deposit", "amount": "50"});

        sqlx::query(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3)",
        )
        .bind(account.id)
        .bind("req-dup")
        .bind(&content)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Duplicate unique_request_id should fail
        let result = sqlx::query(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3)",
        )
        .bind(account.id)
        .bind("req-dup")
        .bind(&content)
        .execute(&mut *tx)
        .await;

        assert!(result.is_err(), "Duplicate unique_request_id should fail");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_balance_change_by_id() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_find_account").await;

        let content = serde_json::json!({"type": "Withdraw", "amount": "25"});

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-find")
        .bind(&content)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        let found =
            sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1")
                .bind(bc.id)
                .fetch_optional(&mut *tx)
                .await
                .unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().unique_request_id, "req-find");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_find_balance_change_by_id_not_found() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let found =
            sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1")
                .bind(999999i64)
                .fetch_optional(&mut *tx)
                .await
                .unwrap();

        assert!(found.is_none());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_mark_balance_change_applied() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_apply_account").await;

        let content = serde_json::json!({"type": "Deposit", "amount": "100"});

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-apply")
        .bind(&content)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert!(!bc.is_applied);

        // Mark as applied
        sqlx::query("UPDATE balance_changes SET is_applied = TRUE WHERE id = $1")
            .bind(bc.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let updated =
            sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1")
                .bind(bc.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert!(updated.is_applied);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_balance_change_for_update_lock() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_lock_account").await;

        let content = serde_json::json!({"type": "Deposit", "amount": "100"});

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-lock")
        .bind(&content)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        // SELECT ... FOR UPDATE should work within the same transaction
        let locked = sqlx::query_as::<_, BalanceChange>(
            "SELECT * FROM balance_changes WHERE id = $1 FOR UPDATE",
        )
        .bind(bc.id)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();

        assert!(locked.is_some());

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_balance_change_spend_token_content() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "bc_spend_account").await;

        let spend = SpendToken {
            input_tokens: 100,
            input_token_price: BigDecimal::from_str("0.000001").unwrap(),
            input_spend_amount: BigDecimal::from_str("0.0001").unwrap(),
            output_tokens: 200,
            output_token_price: BigDecimal::from_str("0.000002").unwrap(),
            output_spend_amount: BigDecimal::from_str("0.0004").unwrap(),
            total_tokens: 300,
            event_id: 42,
        };

        let content = BalanceChangeContent::SpendToken(spend);
        let content_json = serde_json::to_value(&content).unwrap();

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-spend")
        .bind(&content_json)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(bc.content["type"], "SpendToken");
        assert_eq!(bc.content["input_tokens"], 100);
        assert_eq!(bc.content["output_tokens"], 200);
        assert_eq!(bc.content["total_tokens"], 300);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_new_balance_change_from_content() {
        let content = BalanceChangeContent::Deposit {
            amount: BigDecimal::from(100),
        };

        let new_bc =
            NewBalanceChange::from_content(1, "req-from-content".to_string(), &content).unwrap();

        assert_eq!(new_bc.account_id, 1);
        assert_eq!(new_bc.unique_request_id, "req-from-content");
        assert_eq!(new_bc.content["type"], "Deposit");
    }

    #[tokio::test]
    async fn test_new_balance_change_from_withdraw_content() {
        let content = BalanceChangeContent::Withdraw {
            amount: BigDecimal::from(50),
        };

        let new_bc =
            NewBalanceChange::from_content(2, "req-withdraw".to_string(), &content).unwrap();

        assert_eq!(new_bc.account_id, 2);
        assert_eq!(new_bc.content["type"], "Withdraw");
    }

    #[tokio::test]
    async fn test_new_balance_change_from_credit_content() {
        let content = BalanceChangeContent::Credit {
            amount: BigDecimal::from(200),
        };

        let new_bc = NewBalanceChange::from_content(3, "req-credit".to_string(), &content).unwrap();

        assert_eq!(new_bc.account_id, 3);
        assert_eq!(new_bc.content["type"], "Credit");
    }
}

// ============================================================
// Cross-module Integration Tests
// ============================================================

mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_account_with_fund_and_api_key() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Create account
        let account = create_test_account(&mut tx, "integration_account").await;

        // Create fund for account
        let fund = sqlx::query_as::<_, Fund>(
            "INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 1000, 500, 0) RETURNING *",
        )
        .bind(account.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(fund.account_id, account.id);
        assert_eq!(fund.available(), BigDecimal::from(1500));

        // Create API key for account
        let api_key = sqlx::query_as::<_, ApiCredential>(
            "INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("lpx-integration-key")
        .bind("integration test")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(api_key.account_id, Some(account.id));

        // Verify account can be found by API key
        let found_key = sqlx::query_as::<_, ApiCredential>(
            "SELECT * FROM api_credentials WHERE apikey = $1 AND is_active = true",
        )
        .bind("lpx-integration-key")
        .fetch_optional(&mut *tx)
        .await
        .unwrap();

        assert!(found_key.is_some());
        let found_key = found_key.unwrap();

        // Find account from API key
        let found_account = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = $1")
            .bind(found_key.account_id.unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(found_account.name, "integration_account");

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_upstream_with_models_and_session_events() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Create upstream and model
        let upstream =
            create_test_upstream(&mut tx, "integration-ep", "https://api.integration.com/v1").await;
        let model = create_test_model(&mut tx, upstream.id, "gpt-4-integration").await;

        // Create account and API key
        let account = create_test_account(&mut tx, "integration_ep_account").await;
        let api_credential = sqlx::query_as::<_, ApiCredential>(
            "INSERT INTO api_credentials (account_id, apikey, label) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("lpx-integration-ep-key")
        .bind("test")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        // Create session event referencing the model and account
        let event_data = serde_json::json!({
            "type": "chat_completion",
            "model": "gpt-4-integration",
            "input_tokens": 50,
            "output_tokens": 100
        });

        let event = sqlx::query_as::<_, SessionEvent>(
            "INSERT INTO session_events (session_id, session_index, account_id, model_id, api_credential_id, event_data)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind("sess-integration")
        .bind(0)
        .bind(account.id)
        .bind(model.id)
        .bind(api_credential.id)
        .bind(&event_data)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert_eq!(event.account_id, account.id);
        assert_eq!(event.model_id, model.id);
        assert_eq!(event.api_key_id, api_credential.id);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_full_balance_change_workflow() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Create account
        let account = create_test_account(&mut tx, "workflow_account").await;

        // Create fund
        sqlx::query("INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 0, 0, 0)")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Create a deposit balance change
        let deposit_content = BalanceChangeContent::Deposit {
            amount: BigDecimal::from(500),
        };
        let deposit_json = serde_json::to_value(&deposit_content).unwrap();

        let bc = sqlx::query_as::<_, BalanceChange>(
            "INSERT INTO balance_changes (account_id, unique_request_id, content) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(account.id)
        .bind("req-workflow-deposit")
        .bind(&deposit_json)
        .fetch_one(&mut *tx)
        .await
        .unwrap();

        assert!(!bc.is_applied);

        // Simulate applying the balance change: update fund
        sqlx::query("UPDATE funds SET cash = cash + 500 WHERE account_id = $1")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Mark as applied
        sqlx::query("UPDATE balance_changes SET is_applied = TRUE WHERE id = $1")
            .bind(bc.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Verify
        let fund = sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
            .bind(account.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        assert_eq!(fund.cash, BigDecimal::from(500));

        let bc_updated =
            sqlx::query_as::<_, BalanceChange>("SELECT * FROM balance_changes WHERE id = $1")
                .bind(bc.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();

        assert!(bc_updated.is_applied);

        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn test_account_deletion_cascades_to_fund() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let account = create_test_account(&mut tx, "cascade_fund_account").await;

        sqlx::query("INSERT INTO funds (account_id, cash, credit, debt) VALUES ($1, 100, 0, 0)")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        // Delete account - fund should cascade (due to FK constraint)
        sqlx::query("DELETE FROM accounts WHERE id = $1")
            .bind(account.id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let fund = sqlx::query_as::<_, Fund>("SELECT * FROM funds WHERE account_id = $1")
            .bind(account.id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap();

        // Note: funds table has REFERENCES accounts(id) but without ON DELETE CASCADE,
        // so this might fail depending on the actual constraint. If it does, the test
        // documents the expected behavior.
        // If the FK doesn't cascade, the delete of the account would fail instead.
        assert!(
            fund.is_none(),
            "Fund should be removed when account is deleted"
        );

        tx.rollback().await.unwrap();
    }
}
