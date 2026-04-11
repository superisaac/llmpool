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

/// Derive the cname from a fullname: the part after the first "/", or the fullname itself if no "/" present.
fn derive_cname(fullname: &str) -> String {
    if let Some(pos) = fullname.find('/') {
        fullname[pos + 1..].to_string()
    } else {
        fullname.to_string()
    }
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
        "INSERT INTO llm_upstreams (name, api_base, encrypted_api_key, provider, tags, proxies, status, description)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&new_upstream.name)
    .bind(&new_upstream.api_base)
    .bind(&encrypted_key)
    .bind(&new_upstream.provider)
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
pub async fn get_upstream(pool: &DbPool, upstream_id: i64) -> Result<LLMUpstream, sqlx::Error> {
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
    upstream_id: i64,
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
         SET name = $1, api_base = $2, encrypted_api_key = $3, provider = $4, tags = $5, proxies = $6, status = $7, description = $8, updated_at = $9
         WHERE id = $10
         RETURNING *",
    )
    .bind(name)
    .bind(api_base)
    .bind(&encrypted_key)
    .bind(provider)
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

/// Mark an upstream as offline by setting its status to "offline"
pub async fn mark_upstream_offline(pool: &DbPool, upstream_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE llm_upstreams SET status = 'offline', updated_at = NOW() WHERE id = $1")
        .bind(upstream_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark an upstream as online by setting its status to "online"
pub async fn mark_upstream_online(pool: &DbPool, upstream_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE llm_upstreams SET status = 'online', updated_at = NOW() WHERE id = $1")
        .bind(upstream_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// List all upstreams with status = 'offline'
pub async fn list_offline_upstreams(pool: &DbPool) -> Result<Vec<LLMUpstream>, sqlx::Error> {
    let upstreams =
        sqlx::query_as::<_, LLMUpstream>("SELECT * FROM llm_upstreams WHERE status = 'offline'")
            .fetch_all(pool)
            .await?;
    upstreams.into_iter().map(decrypt_upstream).collect()
}

/// Delete an OpenAI upstream
pub async fn delete_upstream(pool: &DbPool, upstream_id: i64) -> Result<u64, sqlx::Error> {
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
    let cname = derive_cname(&new_model.fullname);
    sqlx::query_as::<_, LLMModel>(
        "INSERT INTO llm_models (upstream_id, fullname, cname, features, max_tokens, input_token_price, output_token_price, batch_input_token_price, batch_output_token_price)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *"
    )
    .bind(new_model.upstream_id)
    .bind(&new_model.fullname)
    .bind(&cname)
    .bind(&new_model.features)
    .bind(new_model.max_tokens)
    .bind(&new_model.input_token_price)
    .bind(&new_model.output_token_price)
    .bind(&new_model.batch_input_token_price)
    .bind(&new_model.batch_output_token_price)
    .fetch_one(pool)
    .await
}

/// List all OpenAI models, optionally filtered by a required feature.
/// If `capacity.feature` is set, only models containing that feature are returned.
pub async fn list_models(
    pool: &DbPool,
    capacity: &CapacityOption,
) -> Result<Vec<LLMModel>, sqlx::Error> {
    if let Some(ref feature) = capacity.feature {
        sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE $1 = ANY(features)")
            .bind(feature)
            .fetch_all(pool)
            .await
    } else {
        sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models")
            .fetch_all(pool)
            .await
    }
}

/// List models belonging to a specific upstream
pub async fn list_models_by_upstream(
    pool: &DbPool,
    upstream_id: i64,
) -> Result<Vec<LLMModel>, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE upstream_id = $1")
        .bind(upstream_id)
        .fetch_all(pool)
        .await
}

/// Get an OpenAI model by ID
pub async fn get_model(pool: &DbPool, model_id: i64) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(pool)
        .await
}

/// Get an OpenAI model by ID using a transaction
pub async fn get_model_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    model_id: i64,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(&mut **tx)
        .await
}

/// Find a model by upstream_id and fullname string
pub async fn find_model_by_upstream_and_model_id(
    pool: &DbPool,
    upstream_id: i64,
    model_id_str: &str,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>(
        "SELECT * FROM llm_models WHERE upstream_id = $1 AND fullname = $2",
    )
    .bind(upstream_id)
    .bind(model_id_str)
    .fetch_one(pool)
    .await
}

/// Find a model by upstream name and fullname string
pub async fn find_model_by_upstream_name_and_model_id(
    pool: &DbPool,
    upstream_name: &str,
    model_id_str: &str,
) -> Result<Option<LLMModel>, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>(
        "SELECT m.* FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE e.name = $1 AND m.fullname = $2",
    )
    .bind(upstream_name)
    .bind(model_id_str)
    .fetch_optional(pool)
    .await
}

/// Update an OpenAI model
pub async fn update_model(
    pool: &DbPool,
    model_pk: i64,
    update: &UpdateLLMModel,
) -> Result<LLMModel, sqlx::Error> {
    // Fetch current values first
    let current = get_model(pool, model_pk).await?;

    let (fullname, cname) = if let Some(ref new_fullname) = update.fullname {
        (new_fullname.as_str(), derive_cname(new_fullname))
    } else {
        (current.fullname.as_str(), current.cname.clone())
    };
    let is_active = update.is_active.unwrap_or(current.is_active);
    let features = update.features.as_ref().unwrap_or(&current.features);
    let max_tokens = update.max_tokens.unwrap_or(current.max_tokens);
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
         SET fullname = $1, cname = $2, is_active = $3, features = $4,
             max_tokens = $5,
             input_token_price = $6, output_token_price = $7,
             batch_input_token_price = $8, batch_output_token_price = $9,
             description = $10, updated_at = $11
         WHERE id = $12
         RETURNING *",
    )
    .bind(fullname)
    .bind(&cname)
    .bind(is_active)
    .bind(features)
    .bind(max_tokens)
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

/// Find the first model by cname (short name)
pub async fn find_first_model_by_name(
    pool: &DbPool,
    model_name: &str,
) -> Result<LLMModel, sqlx::Error> {
    sqlx::query_as::<_, LLMModel>("SELECT * FROM llm_models WHERE cname = $1 LIMIT 1")
        .bind(model_name)
        .fetch_one(pool)
        .await
}

/// Filter options for listing models
pub struct ListModelsFilter {
    pub upstream_id: Option<i64>,
    pub upstream_name: Option<String>,
    pub name: Option<String>,
    pub is_active: Option<bool>,
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
        sql.push_str(&format!(" AND m.cname = ${}", param_idx));
    }
    if filter.is_active.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.is_active = ${}", param_idx));
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
    if let Some(ref is_active) = filter.is_active {
        query = query.bind(is_active);
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
        sql.push_str(&format!(" AND m.cname = ${}", param_idx));
    }
    if filter.is_active.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.is_active = ${}", param_idx));
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
    if let Some(ref is_active) = filter.is_active {
        query = query.bind(is_active);
    }
    query = query.bind(limit).bind(offset);
    query.fetch_all(pool).await
}

/// Delete an OpenAI model
pub async fn delete_model(pool: &DbPool, model_pk: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM llm_models WHERE id = $1")
        .bind(model_pk)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get an upstream with all its models
pub async fn get_upstream_with_models(
    pool: &DbPool,
    upstream_id: i64,
) -> Result<(LLMUpstream, Vec<LLMModel>), sqlx::Error> {
    let upstream = get_upstream(pool, upstream_id).await?;
    let models = list_models_by_upstream(pool, upstream_id).await?;
    Ok((upstream, models))
}

/// Get the tags of an upstream by its ID.
pub async fn get_upstream_tags(
    pool: &DbPool,
    upstream_id: i64,
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
    upstream_id: i64,
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
    upstream_id: i64,
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
    pub id: i64,
    pub upstream_id: i64,
    pub fullname: String,
    pub cname: String,
    pub is_active: bool,
    pub features: Vec<String>,
    pub max_tokens: i64,
    pub input_token_price: bigdecimal::BigDecimal,
    pub output_token_price: bigdecimal::BigDecimal,
    pub batch_input_token_price: bigdecimal::BigDecimal,
    pub batch_output_token_price: bigdecimal::BigDecimal,
    pub description: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    // LLMUpstream fields (aliased)
    pub ep_id: i64,
    pub ep_name: String,
    pub ep_api_base: String,
    pub ep_encrypted_api_key: String,
    pub ep_provider: String,
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
            fullname: self.fullname,
            cname: self.cname,
            is_active: self.is_active,
            features: self.features,
            max_tokens: self.max_tokens,
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

/// Find all OpenAI models with a given cname (short name) that match the specified capacity options,
/// along with their associated upstream information (api_key, api_base).
///
/// If `capacity.feature` is set, only models containing that feature string are returned.
pub async fn find_models_by_name_and_capacity(
    pool: &DbPool,
    model_name: &str,
    capacity: &CapacityOption,
) -> Result<Vec<(LLMModel, LLMUpstream)>, sqlx::Error> {
    // Build dynamic query with optional feature filter
    let mut sql = String::from(
        "SELECT m.id, m.upstream_id, m.fullname, m.cname, m.is_active, m.features,
                m.max_tokens, m.input_token_price, m.output_token_price,
                m.batch_input_token_price, m.batch_output_token_price,
                m.description, m.created_at, m.updated_at,
                e.id AS ep_id, e.name AS ep_name, e.api_base AS ep_api_base,
                e.encrypted_api_key AS ep_encrypted_api_key, e.provider AS ep_provider,
                e.tags AS ep_tags, e.proxies AS ep_proxies,
                e.status AS ep_status, e.description AS ep_description,
                e.created_at AS ep_created_at, e.updated_at AS ep_updated_at
         FROM llm_models m
         INNER JOIN llm_upstreams e ON m.upstream_id = e.id
         WHERE m.cname = $1
                AND e.status = 'online'
                AND m.is_active = true ",
    );

    if capacity.feature.is_some() {
        sql.push_str(" AND $2 = ANY(m.features)");
    }

    let mut query = sqlx::query_as::<_, ModelUpstreamRow>(&sql).bind(model_name);

    if let Some(ref feature) = capacity.feature {
        query = query.bind(feature);
    }

    let rows = query.fetch_all(pool).await?;
    rows.into_iter()
        .map(|r| r.into_tuple())
        .collect::<Result<Vec<_>, _>>()
}
