use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::crypto;
use crate::db::DbPool;
use crate::models::{Account, ApiCredential, NewApiCredential};

// ============================================================
// Helper: encrypt/hash apikey
// ============================================================

/// Compute the ellipsed representation of an apikey:
/// first 6 chars + "..." + last 6 chars.
/// If the key is too short, the full key is returned.
fn ellipse_api_key(key: &str) -> String {
    if key.len() <= 12 {
        key.to_string()
    } else {
        format!("{}...{}", &key[..6], &key[key.len() - 6..])
    }
}

/// Compute a SHA-256 hex hash of the plaintext apikey.
/// Used as a fast, non-reversible lookup key.
fn hash_api_key(apikey: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(apikey.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Encrypt a plaintext apikey before storing it in the database.
/// If encryption is not configured, the value is returned as-is.
fn encrypt_api_key(apikey: &str) -> Result<String, sqlx::Error> {
    crypto::encrypt_if_configured(apikey)
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to encrypt apikey: {}", e)))
}

/// Decrypt the `encrypted_api_key` field of an `ApiCredential`, populate `apikey` with the
/// plaintext, and populate `ellipsed_api_key` with the ellipsed representation.
/// If encryption is not configured, the value is returned as-is.
fn decrypt_credential(mut cred: ApiCredential) -> Result<ApiCredential, sqlx::Error> {
    let plaintext = crypto::decrypt_if_configured(&cred.encrypted_api_key).map_err(|e| {
        sqlx::Error::Protocol(format!("Failed to decrypt encrypted_api_key: {}", e))
    })?;
    cred.ellipsed_api_key = ellipse_api_key(&plaintext);
    cred.apikey = plaintext;
    Ok(cred)
}

// ============================================================
// ApiCredential CRUD operations
// ============================================================

/// Find an active API credential by the SHA-256 hash of the apikey string.
/// The hash is computed from the plaintext apikey by the caller.
pub async fn find_active_api_credential_by_api_key_hash(
    pool: &DbPool,
    api_key_hash: &str,
) -> Result<Option<ApiCredential>, sqlx::Error> {
    let cred = sqlx::query_as::<_, ApiCredential>(
        "SELECT * FROM api_credentials WHERE api_key_hash = $1 AND is_active = true",
    )
    .bind(api_key_hash)
    .fetch_optional(pool)
    .await?;
    cred.map(decrypt_credential).transpose()
}

/// Find an active API credential by the plaintext apikey string.
/// Computes the SHA-256 hash internally and delegates to find_active_api_credential_by_api_key_hash.
pub async fn find_active_api_credential_by_apikey(
    pool: &DbPool,
    apikey: &str,
) -> Result<Option<ApiCredential>, sqlx::Error> {
    let key_hash = hash_api_key(apikey);
    find_active_api_credential_by_api_key_hash(pool, &key_hash).await
}

/// Generate a random API key string with the prefix "lpx-"
/// Uses UUIDv7 algorithm (time-ordered with random bits) and outputs as hex string
fn generate_api_credential() -> String {
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

/// Count total number of API keys for a account
pub async fn count_api_credentials_by_account(
    pool: &DbPool,
    account_id: i32,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_credentials WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List API keys for a account with pagination.
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_api_credentials_by_account_paginated(
    pool: &DbPool,
    account_id: i32,
    offset: i64,
    limit: i64,
) -> Result<Vec<ApiCredential>, sqlx::Error> {
    let creds = sqlx::query_as::<_, ApiCredential>(
        "SELECT * FROM api_credentials WHERE account_id = $1 ORDER BY id ASC LIMIT $2 OFFSET $3",
    )
    .bind(account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    creds.into_iter().map(decrypt_credential).collect()
}

/// Find an API credential by the SHA-256 hash of the apikey string (active or inactive).
pub async fn find_api_credential_by_apikey(
    pool: &DbPool,
    apikey: &str,
) -> Result<Option<ApiCredential>, sqlx::Error> {
    let key_hash = hash_api_key(apikey);
    let cred =
        sqlx::query_as::<_, ApiCredential>("SELECT * FROM api_credentials WHERE api_key_hash = $1")
            .bind(&key_hash)
            .fetch_optional(pool)
            .await?;
    cred.map(decrypt_credential).transpose()
}

/// Deactivate an API credential by setting is_active = false (soft delete).
/// Looks up by SHA-256 hash of the apikey.
/// Returns the updated credential, or RowNotFound if it doesn't exist.
pub async fn deactivate_api_credential(
    pool: &DbPool,
    apikey: &str,
) -> Result<ApiCredential, sqlx::Error> {
    let key_hash = hash_api_key(apikey);
    let cred = sqlx::query_as::<_, ApiCredential>(
        "UPDATE api_credentials SET is_active = false, updated_at = NOW() WHERE api_key_hash = $1 RETURNING *",
    )
    .bind(&key_hash)
    .fetch_one(pool)
    .await?;
    decrypt_credential(cred)
}

/// Create a new API key for a account
pub async fn create_api_credential_for_account(
    pool: &DbPool,
    account_id: i32,
    label: &str,
) -> Result<ApiCredential, sqlx::Error> {
    let plaintext = generate_api_credential();
    let encrypted_key = encrypt_api_key(&plaintext)?;
    let key_hash = hash_api_key(&plaintext);
    let new_key = NewApiCredential {
        account_id: Some(account_id),
        apikey: plaintext,
        label: label.to_string(),
        expires_at: None,
    };
    let cred = sqlx::query_as::<_, ApiCredential>(
        "INSERT INTO api_credentials (account_id, encrypted_api_key, api_key_hash, label, expires_at)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(new_key.account_id)
    .bind(&encrypted_key)
    .bind(&key_hash)
    .bind(&new_key.label)
    .bind(new_key.expires_at)
    .fetch_one(pool)
    .await?;
    decrypt_credential(cred)
}
