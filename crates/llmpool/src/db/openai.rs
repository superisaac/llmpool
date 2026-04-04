use sqlx::PgPool;

use crate::crypto;
use crate::models::*;

pub type DbPool = PgPool;

// ============================================================
// Helper: decrypt api_key in an LLMUpstream after reading from DB
// ============================================================

/// Compute the ellipsed representation of an api_key:
/// first 6 chars + "..." + last 6 chars.
/// If the key is too short, the full key is returned.
fn ellipse_api_key(key: &str) -> String {
    if key.len() <= 12 {
        key.to_string()
    } else {
        format!("{}...{}", &key[..6], &key[key.len() - 6..])
    }
}

/// Decrypt the `encrypted_api_key` field of an `LLMUpstream`, populate `api_key` with the
/// plaintext, and populate `ellipsed_api_key` with the ellipsed representation.
/// If encryption is not configured, the value is returned as-is.
fn decrypt_upstream(mut upstream: LLMUpstream) -> Result<LLMUpstream, sqlx::Error> {
    let plaintext = crypto::decrypt_if_configured(&upstream.encrypted_api_key).map_err(|e| {
        sqlx::Error::Protocol(format!("Failed to decrypt encrypted_api_key: {}", e))
    })?;
    upstream.ellipsed_api_key = ellipse_api_key(&plaintext);
    upstream.api_key = plaintext;
    Ok(upstream)
}

/// Encrypt a plaintext api_key before storing it in the database.
/// If encryption is not configured, the value is returned as-is.
fn encrypt_api_key(api_key: &str) -> Result<String, sqlx::Error> {
    crypto::encrypt_if_configured(api_key)
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to encrypt api_key: {}", e)))
}

// ============================================================
// LLMUpstream CRUD operations
// ============================================================

/// Create a new OpenAI upstream
pub async fn create_upstream(
    pool: &DbPool,
    new_upstream: &NewLLMUpstream,
) -> Result<LLMUpstream, sqlx::Error> {
    let encrypted_key = encrypt_api_key(&new_upstream.api_key)?;
    let upstream = sqlx::query_as::<_, LLMUpstream>(
        "INSERT INTO llm_upstreams (name, api_base, encrypted_api_key, provider, has_responses_api, tags, proxies, status, description)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(&new_upstream.name)
    .bind(&new_upstream.api_base)
    .bind(&encrypted_key)
    .bind(&new_upstream.provider)
    .bind(new_upstream.has_responses_api)
    .bind(&new_upstream.tags)
    .bind(&new_upstream.proxies)
    .bind(&new_upstream.status)
    .bind(&new_upstream.description)
    .fetch_one(pool)
    .await?;
    decrypt_upstream(upstream)
}

/// List all OpenAI upstreams (with decrypted api_keys)
pub async fn list_upstreams(pool: &DbPool) -> Result<Vec<LLMUpstream>, sqlx::Error> {
    let upstreams = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams")
        .fetch_all(pool)
        .await?;
    upstreams.into_iter().map(decrypt_upstream).collect()
}

/// Count total number of OpenAI upstreams
pub async fn count_upstreams(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_upstreams")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List OpenAI upstreams with pagination (with decrypted api_keys).
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_upstreams_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<LLMUpstream>, sqlx::Error> {
    let upstreams = sqlx::query_as::<_, LLMUpstream>(
        "SELECT * FROM llm_upstreams ORDER BY id ASC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    upstreams.into_iter().map(decrypt_upstream).collect()
}

/// Get an OpenAI upstream by ID (with decrypted api_key)
pub async fn get_upstream(pool: &DbPool, upstream_id: i32) -> Result<LLMUpstream, sqlx::Error> {
    let upstream = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE id = $1")
        .bind(upstream_id)
        .fetch_one(pool)
        .await?;
    decrypt_upstream(upstream)
}

/// Get an OpenAI upstream by name (with decrypted api_key)
pub async fn get_upstream_by_name(pool: &DbPool, name: &str) -> Result<LLMUpstream, sqlx::Error> {
    let upstream = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await?;
    decrypt_upstream(upstream)
}

/// Get an OpenAI upstream by api_base (with decrypted api_key)
pub async fn get_upstream_by_api_base(
    pool: &DbPool,
    api_base: &str,
) -> Result<LLMUpstream, sqlx::Error> {
    let upstream =
        sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE api_base = $1")
            .bind(api_base)
            .fetch_one(pool)
            .await?;
    decrypt_upstream(upstream)
}

/// Update an OpenAI upstream
pub async fn update_upstream(
    pool: &DbPool,
    upstream_id: i32,
    update: &UpdateLLMUpstream,
) -> Result<LLMUpstream, sqlx::Error> {
    // Fetch current values first (already decrypted by get_upstream)
    let current = get_upstream(pool, upstream_id).await?;

    let name = update.name.as_deref().unwrap_or(&current.name);
    let api_base = update.api_base.as_deref().unwrap_or(&current.api_base);
    // If a new api_key is provided, encrypt it; otherwise re-encrypt the current (decrypted) key
    let plaintext_key = update.api_key.as_deref().unwrap_or(&current.api_key);
    let encrypted_key = encrypt_api_key(plaintext_key)?;
    let provider = update.provider.as_deref().unwrap_or(&current.provider);
    let has_responses_api = update
        .has_responses_api
        .unwrap_or(current.has_responses_api);
    let tags = update.tags.as_ref().unwrap_or(&current.tags);
    let proxies = update.proxies.as_ref().unwrap_or(&current.proxies);
    let status = update.status.as_deref().unwrap_or(&current.status);
    let description = update
        .description
        .as_deref()
        .unwrap_or(&current.description);
    let updated_at = update.updated_at.unwrap_or(current.updated_at);

    let upstream = sqlx::query_as::<_, LLMUpstream>(
        "UPDATE llm_upstreams
         SET name = $1, api_base = $2, encrypted_api_key = $3, provider = $4, has_responses_api = $5, tags = $6, proxies = $7, status = $8, description = $9, updated_at = $10
         WHERE id = $11
         RETURNING *",
    )
    .bind(name)
    .bind(api_base)
    .bind(&encrypted_key)
    .bind(provider)
    .bind(has_responses_api)
    .bind(tags)
    .bind(proxies)
    .bind(status)
    .bind(description)
    .bind(updated_at)
    .bind(upstream_id)
    .fetch_one(pool)
    .await?;
    decrypt_upstream(upstream)
}

/// Delete an OpenAI upstream
pub async fn delete_upstream(pool: &DbPool, upstream_id: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM llm_upstreams WHERE id = $1")
        .bind(upstream_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================
// LLMModel CRUD operations
// ============================================================

/// Create a new OpenAI model
pub async fn create_model(pool: &DbPool, new_model: &NewLLMModel) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>(
        "INSERT INTO llm_models (upstream_id, model_id, has_image_generation, has_speech, has_chat_completion, has_embedding, input_token_price, output_token_price, batch_input_token_price, batch_output_token_price)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *"
    )
    .bind(new_model.upstream_id)
    .bind(&new_model.model_id)
    .bind(new_model.has_image_generation)
    .bind(new_model.has_speech)
    .bind(new_model.has_chat_completion)
    .bind(new_model.has_embedding)
    .bind(&new_model.input_token_price)
    .bind(&new_model.output_token_price)
    .bind(&new_model.batch_input_token_price)
    .bind(&new_model.batch_output_token_price)
    .fetch_one(pool)
    .await
}

/// List all OpenAI models
pub async fn list_models(pool: &DbPool) -> Result<Vec<LLMModel>, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models")
        .fetch_all(pool)
        .await
}

/// List models belonging to a specific upstream
pub async fn list_models_by_upstream(
    pool: &DbPool,
    upstream_id: i32,
) -> Result<Vec<LLMModel>, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE upstream_id = $1")
        .bind(upstream_id)
        .fetch_all(pool)
        .await
}

/// Get an OpenAI model by ID
pub async fn get_model(pool: &DbPool, model_id: i32) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(pool)
        .await
}

/// Get an OpenAI model by ID using a transaction
pub async fn get_model_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    model_id: i32,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(&mut **tx)
        .await
}

/// Find a model by upstream_id and model_id string
pub async fn find_model_by_upstream_and_model_id(
    pool: &DbPool,
    upstream_id: i32,
    model_id_str: &str,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>(
        "SELECT * FROM llm_models WHERE upstream_id = $1 AND model_id = $2",
    )
    .bind(upstream_id)
    .bind(model_id_str)
    .fetch_one(pool)
    .await
}

/// Find a model by upstream name and model_id string
pub async fn find_model_by_upstream_name_and_model_id(
    pool: &DbPool,
    upstream_name: &str,
    model_id_str: &str,
) -> Result<Option<LLMModel>, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>(
        "SELECT m.* FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE e.name = $1 AND m.model_id = $2",
    )
    .bind(upstream_name)
    .bind(model_id_str)
    .fetch_optional(pool)
    .await
}

/// Update an OpenAI model
pub async fn update_model(
    pool: &DbPool,
    model_pk: i32,
    update: &UpdateLLMModel,
) -> Result<LLMModel, sqlx::Error> {
    // Fetch current values first
    let current = get_model(pool, model_pk).await?;

    let model_id = update.model_id.as_deref().unwrap_or(&current.model_id);
    let has_image_generation = update
        .has_image_generation
        .unwrap_or(current.has_image_generation);
    let has_speech = update.has_speech.unwrap_or(current.has_speech);
    let has_chat_completion = update
        .has_chat_completion
        .unwrap_or(current.has_chat_completion);
    let has_embedding = update.has_embedding.unwrap_or(current.has_embedding);
    let input_token_price = update
        .input_token_price
        .as_ref()
        .unwrap_or(&current.input_token_price);
    let output_token_price = update
        .output_token_price
        .as_ref()
        .unwrap_or(&current.output_token_price);
    let batch_input_token_price = update
        .batch_input_token_price
        .as_ref()
        .unwrap_or(&current.batch_input_token_price);
    let batch_output_token_price = update
        .batch_output_token_price
        .as_ref()
        .unwrap_or(&current.batch_output_token_price);
    let description = update
        .description
        .as_deref()
        .unwrap_or(&current.description);
    let updated_at = update.updated_at.unwrap_or(current.updated_at);

    sqlx::query_as::<_, LLMModel>(
        "UPDATE llm_models
         SET model_id = $1, has_image_generation = $2, has_speech = $3, has_chat_completion = $4,
             has_embedding = $5, input_token_price = $6, output_token_price = $7,
             batch_input_token_price = $8, batch_output_token_price = $9,
             description = $10, updated_at = $11
         WHERE id = $12
         RETURNING *",
    )
    .bind(model_id)
    .bind(has_image_generation)
    .bind(has_speech)
    .bind(has_chat_completion)
    .bind(has_embedding)
    .bind(input_token_price)
    .bind(output_token_price)
    .bind(batch_input_token_price)
    .bind(batch_output_token_price)
    .bind(description)
    .bind(updated_at)
    .bind(model_pk)
    .fetch_one(pool)
    .await
}

/// Find the first model by model_id string (name)
pub async fn find_first_model_by_name(
    pool: &DbPool,
    model_name: &str,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE model_id = $1 LIMIT 1")
        .bind(model_name)
        .fetch_one(pool)
        .await
}

/// Filter options for listing models
pub struct ListModelsFilter {
    pub upstream_id: Option<i32>,
    pub upstream_name: Option<String>,
    pub name: Option<String>,
}

/// Count models matching the given filters
pub async fn count_models_filtered(
    pool: &DbPool,
    filter: &ListModelsFilter,
) -> Result<i64, sqlx::Error> {
    let mut sql = String::from(
        "SELECT COUNT(*) FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE 1=1",
    );
    let mut param_idx = 0u32;
    if filter.upstream_id.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.upstream_id = ${}", param_idx));
    }
    if filter.upstream_name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND e.name = ${}", param_idx));
    }
    if filter.name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.model_id = ${}", param_idx));
    }
    let _ = param_idx;

    let mut query = sqlx::query_as::<_, (i64,)>(&sql);
    if let Some(ref eid) = filter.upstream_id {
        query = query.bind(eid);
    }
    if let Some(ref ename) = filter.upstream_name {
        query = query.bind(ename);
    }
    if let Some(ref name) = filter.name {
        query = query.bind(name);
    }
    let row = query.fetch_one(pool).await?;
    Ok(row.0)
}

/// List models matching the given filters with pagination
pub async fn list_models_filtered_paginated(
    pool: &DbPool,
    filter: &ListModelsFilter,
    offset: i64,
    limit: i64,
) -> Result<Vec<LLMModel>, sqlx::Error> {
    let mut sql = String::from(
        "SELECT m.* FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE 1=1",
    );
    let mut param_idx = 0u32;
    if filter.upstream_id.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.upstream_id = ${}", param_idx));
    }
    if filter.upstream_name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND e.name = ${}", param_idx));
    }
    if filter.name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.model_id = ${}", param_idx));
    }
    param_idx += 1;
    let limit_idx = param_idx;
    param_idx += 1;
    let offset_idx = param_idx;
    sql.push_str(&format!(
        " ORDER BY m.id ASC LIMIT ${} OFFSET ${}",
        limit_idx, offset_idx
    ));

    let mut query = sqlx::query_as::<_, LLMModel>(&sql);
    if let Some(ref eid) = filter.upstream_id {
        query = query.bind(eid);
    }
    if let Some(ref ename) = filter.upstream_name {
        query = query.bind(ename);
    }
    if let Some(ref name) = filter.name {
        query = query.bind(name);
    }
    query = query.bind(limit).bind(offset);
    query.fetch_all(pool).await
}

/// Delete an OpenAI model
pub async fn delete_model(pool: &DbPool, model_pk: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM llm_models WHERE id = $1")
        .bind(model_pk)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get an upstream with all its models
pub async fn get_upstream_with_models(
    pool: &DbPool,
    upstream_id: i32,
) -> Result<(LLMUpstream, Vec<LLMModel>), sqlx::Error> {
    let upstream = get_upstream(pool, upstream_id).await?;
    let models = list_models_by_upstream(pool, upstream_id).await?;
    Ok((upstream, models))
}

/// Get the tags of an upstream by its ID.
pub async fn get_upstream_tags(
    pool: &DbPool,
    upstream_id: i32,
) -> Result<Vec<String>, sqlx::Error> {
    let upstream = sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE id = $1")
        .bind(upstream_id)
        .fetch_one(pool)
        .await?;
    Ok(upstream.tags)
}

/// Add a tag to an upstream. Uses array_append and ensures no duplicates.
/// Returns the updated upstream.
pub async fn add_upstream_tag(
    pool: &DbPool,
    upstream_id: i32,
    tag: &str,
) -> Result<LLMUpstream, sqlx::Error> {
    // Use array_append only if the tag is not already present
    let upstream = sqlx::query_as::<_, LLMUpstream>(
        "UPDATE llm_upstreams
         SET tags = CASE
             WHEN $2 = ANY(tags) THEN tags
             ELSE array_append(tags, $2)
         END,
         updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(upstream_id)
    .bind(tag)
    .fetch_one(pool)
    .await?;
    decrypt_upstream(upstream)
}

/// Remove a tag from an upstream. Uses array_remove.
/// Returns the updated upstream.
pub async fn remove_upstream_tag(
    pool: &DbPool,
    upstream_id: i32,
    tag: &str,
) -> Result<LLMUpstream, sqlx::Error> {
    let upstream = sqlx::query_as::<_, LLMUpstream>(
        "UPDATE llm_upstreams
         SET tags = array_remove(tags, $2),
         updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(upstream_id)
    .bind(tag)
    .fetch_one(pool)
    .await?;
    decrypt_upstream(upstream)
}

/// Find all OpenAI upstreams that contain the given tag in their tags array.
/// Returns upstreams with decrypted api_keys.
pub async fn find_upstreams_by_tag(
    pool: &DbPool,
    tag: &str,
) -> Result<Vec<LLMUpstream>, sqlx::Error> {
    let upstreams =
        sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE $1 = ANY(tags)")
            .bind(tag)
            .fetch_all(pool)
            .await?;
    upstreams.into_iter().map(decrypt_upstream).collect()
}

/// A helper struct for the joined query result of LLMModel + LLMUpstream
#[derive(Debug, Clone, sqlx::FromRow)]
struct ModelUpstreamRow {
    // LLMModel fields
    pub id: i32,
    pub upstream_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: bigdecimal::BigDecimal,
    pub output_token_price: bigdecimal::BigDecimal,
    pub batch_input_token_price: bigdecimal::BigDecimal,
    pub batch_output_token_price: bigdecimal::BigDecimal,
    pub description: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    // LLMUpstream fields (aliased)
    pub ep_id: i32,
    pub ep_name: String,
    pub ep_api_base: String,
    pub ep_encrypted_api_key: String,
    pub ep_provider: String,
    pub ep_has_responses_api: bool,
    pub ep_tags: Vec<String>,
    pub ep_proxies: Vec<String>,
    pub ep_status: String,
    pub ep_description: String,
    pub ep_created_at: chrono::NaiveDateTime,
    pub ep_updated_at: chrono::NaiveDateTime,
}

impl ModelUpstreamRow {
    fn into_tuple(self) -> Result<(LLMModel, LLMUpstream), sqlx::Error> {
        let model = LLMModel {
            id: self.id,
            upstream_id: self.upstream_id,
            model_id: self.model_id,
            has_image_generation: self.has_image_generation,
            has_speech: self.has_speech,
            has_chat_completion: self.has_chat_completion,
            has_embedding: self.has_embedding,
            input_token_price: self.input_token_price,
            output_token_price: self.output_token_price,
            batch_input_token_price: self.batch_input_token_price,
            batch_output_token_price: self.batch_output_token_price,
            description: self.description,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        let upstream = LLMUpstream {
            id: self.ep_id,
            name: self.ep_name,
            api_base: self.ep_api_base,
            encrypted_api_key: self.ep_encrypted_api_key,
            ellipsed_api_key: String::new(), // will be populated by decrypt_upstream
            api_key: String::new(),          // will be populated by decrypt_upstream
            provider: self.ep_provider,
            has_responses_api: self.ep_has_responses_api,
            tags: self.ep_tags,
            proxies: self.ep_proxies,
            status: self.ep_status,
            description: self.ep_description,
            created_at: self.ep_created_at,
            updated_at: self.ep_updated_at,
        };
        // Decrypt the api_key from the joined upstream data
        let upstream = decrypt_upstream(upstream)?;
        Ok((model, upstream))
    }
}

/// Find all OpenAI models with a given model_id (name) that match the specified capacity options,
/// along with their associated upstream information (api_key, api_base).
///
/// Only capacity fields set to `Some(true)` in `capacity` will be used as filters.
pub async fn find_models_by_name_and_capacity(
    pool: &DbPool,
    model_name: &str,
    capacity: &CapacityOption,
) -> Result<Vec<(LLMModel, LLMUpstream)>, sqlx::Error> {
    // Build dynamic query with optional capacity filters
    let mut sql = String::from(
        "SELECT m.id, m.upstream_id, m.model_id, m.has_image_generation, m.has_speech,
                m.has_chat_completion, m.has_embedding, m.input_token_price, m.output_token_price,
                m.batch_input_token_price, m.batch_output_token_price,
                m.description, m.created_at, m.updated_at,
                e.id AS ep_id, e.name AS ep_name, e.api_base AS ep_api_base,
                e.encrypted_api_key AS ep_encrypted_api_key, e.provider AS ep_provider,
                e.has_responses_api AS ep_has_responses_api,
                e.tags AS ep_tags, e.proxies AS ep_proxies,
                e.status AS ep_status, e.description AS ep_description,
                e.created_at AS ep_created_at, e.updated_at AS ep_updated_at
         FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE m.model_id = $1",
    );

    let mut param_idx = 2u32;
    let mut conditions = Vec::new();

    if capacity.has_chat_completion == Some(true) {
        conditions.push(format!("m.has_chat_completion = ${}", param_idx));
        param_idx += 1;
    }
    if capacity.has_embedding == Some(true) {
        conditions.push(format!("m.has_embedding = ${}", param_idx));
        param_idx += 1;
    }
    if capacity.has_image_generation == Some(true) {
        conditions.push(format!("m.has_image_generation = ${}", param_idx));
        param_idx += 1;
    }
    if capacity.has_speech == Some(true) {
        conditions.push(format!("m.has_speech = ${}", param_idx));
        let _ = param_idx; // suppress unused warning
    }

    for cond in &conditions {
        sql.push_str(" AND ");
        sql.push_str(cond);
    }

    let mut query = sqlx::query_as::<_, ModelUpstreamRow>(&sql).bind(model_name);

    // Bind the `true` values for each capacity filter
    if capacity.has_chat_completion == Some(true) {
        query = query.bind(true);
    }
    if capacity.has_embedding == Some(true) {
        query = query.bind(true);
    }
    if capacity.has_image_generation == Some(true) {
        query = query.bind(true);
    }
    if capacity.has_speech == Some(true) {
        query = query.bind(true);
    }

    let rows = query.fetch_all(pool).await?;
    rows.into_iter()
        .map(|r| r.into_tuple())
        .collect::<Result<Vec<_>, _>>()
}
