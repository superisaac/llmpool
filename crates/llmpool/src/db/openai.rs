use sqlx::PgPool;

use crate::crypto;
use crate::models::*;

pub type DbPool = PgPool;

// ============================================================
// Helper: decrypt api_key in an OpenAIEndpoint after reading from DB
// ============================================================

/// Decrypt the `api_key` field of an `OpenAIEndpoint`.
/// If encryption is not configured, the value is returned as-is.
fn decrypt_endpoint(mut endpoint: OpenAIEndpoint) -> Result<OpenAIEndpoint, sqlx::Error> {
    endpoint.api_key = crypto::decrypt_if_configured(&endpoint.api_key)
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to decrypt api_key: {}", e)))?;
    Ok(endpoint)
}

/// Encrypt a plaintext api_key before storing it in the database.
/// If encryption is not configured, the value is returned as-is.
fn encrypt_api_key(api_key: &str) -> Result<String, sqlx::Error> {
    crypto::encrypt_if_configured(api_key)
        .map_err(|e| sqlx::Error::Protocol(format!("Failed to encrypt api_key: {}", e)))
}

// ============================================================
// OpenAIEndpoint CRUD operations
// ============================================================

/// Create a new OpenAI endpoint
pub async fn create_endpoint(
    pool: &DbPool,
    new_endpoint: &NewOpenAIEndpoint,
) -> Result<OpenAIEndpoint, sqlx::Error> {
    let encrypted_key = encrypt_api_key(&new_endpoint.api_key)?;
    let endpoint = sqlx::query_as::<_, OpenAIEndpoint>(
        "INSERT INTO openai_endpoints (name, api_base, api_key, has_responses_api, tags, proxies, status, description)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&new_endpoint.name)
    .bind(&new_endpoint.api_base)
    .bind(&encrypted_key)
    .bind(new_endpoint.has_responses_api)
    .bind(&new_endpoint.tags)
    .bind(&new_endpoint.proxies)
    .bind(&new_endpoint.status)
    .bind(&new_endpoint.description)
    .fetch_one(pool)
    .await?;
    decrypt_endpoint(endpoint)
}

/// List all OpenAI endpoints (with decrypted api_keys)
pub async fn list_endpoints(pool: &DbPool) -> Result<Vec<OpenAIEndpoint>, sqlx::Error> {
    let endpoints = sqlx::query_as::<_, OpenAIEndpoint>("SELECT * FROM openai_endpoints")
        .fetch_all(pool)
        .await?;
    endpoints.into_iter().map(decrypt_endpoint).collect()
}

/// Count total number of OpenAI endpoints
pub async fn count_endpoints(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM openai_endpoints")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// List OpenAI endpoints with pagination (with decrypted api_keys).
/// `offset` is the number of rows to skip, `limit` is the max number of rows to return.
pub async fn list_endpoints_paginated(
    pool: &DbPool,
    offset: i64,
    limit: i64,
) -> Result<Vec<OpenAIEndpoint>, sqlx::Error> {
    let endpoints = sqlx::query_as::<_, OpenAIEndpoint>(
        "SELECT * FROM openai_endpoints ORDER BY id ASC LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    endpoints.into_iter().map(decrypt_endpoint).collect()
}

/// Get an OpenAI endpoint by ID (with decrypted api_key)
pub async fn get_endpoint(pool: &DbPool, endpoint_id: i32) -> Result<OpenAIEndpoint, sqlx::Error> {
    let endpoint =
        sqlx::query_as::<_, OpenAIEndpoint>("SELECT * FROM openai_endpoints WHERE id = $1")
            .bind(endpoint_id)
            .fetch_one(pool)
            .await?;
    decrypt_endpoint(endpoint)
}

/// Get an OpenAI endpoint by api_base (with decrypted api_key)
pub async fn get_endpoint_by_api_base(
    pool: &DbPool,
    api_base: &str,
) -> Result<OpenAIEndpoint, sqlx::Error> {
    let endpoint =
        sqlx::query_as::<_, OpenAIEndpoint>("SELECT * FROM openai_endpoints WHERE api_base = $1")
            .bind(api_base)
            .fetch_one(pool)
            .await?;
    decrypt_endpoint(endpoint)
}

/// Update an OpenAI endpoint
pub async fn update_endpoint(
    pool: &DbPool,
    endpoint_id: i32,
    update: &UpdateOpenAIEndpoint,
) -> Result<OpenAIEndpoint, sqlx::Error> {
    // Fetch current values first (already decrypted by get_endpoint)
    let current = get_endpoint(pool, endpoint_id).await?;

    let name = update.name.as_deref().unwrap_or(&current.name);
    let api_base = update.api_base.as_deref().unwrap_or(&current.api_base);
    // If a new api_key is provided, encrypt it; otherwise re-encrypt the current (decrypted) key
    let plaintext_key = update.api_key.as_deref().unwrap_or(&current.api_key);
    let encrypted_key = encrypt_api_key(plaintext_key)?;
    let has_responses_api = update
        .has_responses_api
        .unwrap_or(current.has_responses_api);
    let tags = update.tags.as_ref().unwrap_or(&current.tags);
    let proxies = update.proxies.as_ref().unwrap_or(&current.proxies);
    let status = update.status.as_deref().unwrap_or(&current.status);
    let description = update.description.as_deref().unwrap_or(&current.description);
    let updated_at = update.updated_at.unwrap_or(current.updated_at);

    let endpoint = sqlx::query_as::<_, OpenAIEndpoint>(
        "UPDATE openai_endpoints
         SET name = $1, api_base = $2, api_key = $3, has_responses_api = $4, tags = $5, proxies = $6, status = $7, description = $8, updated_at = $9
         WHERE id = $10
         RETURNING *",
    )
    .bind(name)
    .bind(api_base)
    .bind(&encrypted_key)
    .bind(has_responses_api)
    .bind(tags)
    .bind(proxies)
    .bind(status)
    .bind(description)
    .bind(updated_at)
    .bind(endpoint_id)
    .fetch_one(pool)
    .await?;
    decrypt_endpoint(endpoint)
}

/// Delete an OpenAI endpoint
pub async fn delete_endpoint(pool: &DbPool, endpoint_id: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM openai_endpoints WHERE id = $1")
        .bind(endpoint_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================
// OpenAIModel CRUD operations
// ============================================================

/// Create a new OpenAI model
pub async fn create_model(
    pool: &DbPool,
    new_model: &NewOpenAIModel,
) -> Result<OpenAIModel, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>(
        "INSERT INTO openai_models (endpoint_id, model_id, has_image_generation, has_speech, has_chat_completion, has_embedding, input_token_price, output_token_price)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *"
    )
    .bind(new_model.endpoint_id)
    .bind(&new_model.model_id)
    .bind(new_model.has_image_generation)
    .bind(new_model.has_speech)
    .bind(new_model.has_chat_completion)
    .bind(new_model.has_embedding)
    .bind(&new_model.input_token_price)
    .bind(&new_model.output_token_price)
    .fetch_one(pool)
    .await
}

/// List all OpenAI models
pub async fn list_models(pool: &DbPool) -> Result<Vec<OpenAIModel>, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>("SELECT * FROM openai_models")
        .fetch_all(pool)
        .await
}

/// List models belonging to a specific endpoint
pub async fn list_models_by_endpoint(
    pool: &DbPool,
    endpoint_id: i32,
) -> Result<Vec<OpenAIModel>, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>("SELECT * FROM openai_models WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .fetch_all(pool)
        .await
}

/// Get an OpenAI model by ID
pub async fn get_model(pool: &DbPool, model_id: i32) -> Result<OpenAIModel, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>("SELECT * FROM openai_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(pool)
        .await
}

/// Get an OpenAI model by ID using a transaction
pub async fn get_model_with_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    model_id: i32,
) -> Result<OpenAIModel, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>("SELECT * FROM openai_models WHERE id = $1")
        .bind(model_id)
        .fetch_one(&mut **tx)
        .await
}

/// Find a model by endpoint_id and model_id string
pub async fn find_model_by_endpoint_and_model_id(
    pool: &DbPool,
    endpoint_id: i32,
    model_id_str: &str,
) -> Result<OpenAIModel, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>(
        "SELECT * FROM openai_models WHERE endpoint_id = $1 AND model_id = $2",
    )
    .bind(endpoint_id)
    .bind(model_id_str)
    .fetch_one(pool)
    .await
}

/// Update an OpenAI model
pub async fn update_model(
    pool: &DbPool,
    model_pk: i32,
    update: &UpdateOpenAIModel,
) -> Result<OpenAIModel, sqlx::Error> {
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
    let description = update
        .description
        .as_deref()
        .unwrap_or(&current.description);
    let updated_at = update.updated_at.unwrap_or(current.updated_at);

    sqlx::query_as::<_, OpenAIModel>(
        "UPDATE openai_models
         SET model_id = $1, has_image_generation = $2, has_speech = $3, has_chat_completion = $4,
             has_embedding = $5, input_token_price = $6, output_token_price = $7,
             description = $8, updated_at = $9
         WHERE id = $10
         RETURNING *",
    )
    .bind(model_id)
    .bind(has_image_generation)
    .bind(has_speech)
    .bind(has_chat_completion)
    .bind(has_embedding)
    .bind(input_token_price)
    .bind(output_token_price)
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
) -> Result<OpenAIModel, sqlx::Error> {
    sqlx::query_as::<_, OpenAIModel>("SELECT * FROM openai_models WHERE model_id = $1 LIMIT 1")
        .bind(model_name)
        .fetch_one(pool)
        .await
}

/// Filter options for listing models
pub struct ListModelsFilter {
    pub endpoint_id: Option<i32>,
    pub endpoint_name: Option<String>,
    pub name: Option<String>,
}

/// Count models matching the given filters
pub async fn count_models_filtered(
    pool: &DbPool,
    filter: &ListModelsFilter,
) -> Result<i64, sqlx::Error> {
    let mut sql = String::from(
        "SELECT COUNT(*) FROM openai_models m
         INNER JOIN openai_endpoints e ON m.endpoint_id = e.id
         WHERE 1=1",
    );
    let mut param_idx = 0u32;
    if filter.endpoint_id.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.endpoint_id = ${}", param_idx));
    }
    if filter.endpoint_name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND e.name = ${}", param_idx));
    }
    if filter.name.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.model_id = ${}", param_idx));
    }
    let _ = param_idx;

    let mut query = sqlx::query_as::<_, (i64,)>(&sql);
    if let Some(ref eid) = filter.endpoint_id {
        query = query.bind(eid);
    }
    if let Some(ref ename) = filter.endpoint_name {
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
) -> Result<Vec<OpenAIModel>, sqlx::Error> {
    let mut sql = String::from(
        "SELECT m.* FROM openai_models m
         INNER JOIN openai_endpoints e ON m.endpoint_id = e.id
         WHERE 1=1",
    );
    let mut param_idx = 0u32;
    if filter.endpoint_id.is_some() {
        param_idx += 1;
        sql.push_str(&format!(" AND m.endpoint_id = ${}", param_idx));
    }
    if filter.endpoint_name.is_some() {
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

    let mut query = sqlx::query_as::<_, OpenAIModel>(&sql);
    if let Some(ref eid) = filter.endpoint_id {
        query = query.bind(eid);
    }
    if let Some(ref ename) = filter.endpoint_name {
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
    let result = sqlx::query("DELETE FROM openai_models WHERE id = $1")
        .bind(model_pk)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get an endpoint with all its models
pub async fn get_endpoint_with_models(
    pool: &DbPool,
    endpoint_id: i32,
) -> Result<(OpenAIEndpoint, Vec<OpenAIModel>), sqlx::Error> {
    let endpoint = get_endpoint(pool, endpoint_id).await?;
    let models = list_models_by_endpoint(pool, endpoint_id).await?;
    Ok((endpoint, models))
}

/// Find all OpenAI endpoints that contain the given tag in their tags array.
/// Returns endpoints with decrypted api_keys.
pub async fn find_endpoints_by_tag(
    pool: &DbPool,
    tag: &str,
) -> Result<Vec<OpenAIEndpoint>, sqlx::Error> {
    let endpoints =
        sqlx::query_as::<_, OpenAIEndpoint>("SELECT * FROM openai_endpoints WHERE $1 = ANY(tags)")
            .bind(tag)
            .fetch_all(pool)
            .await?;
    endpoints.into_iter().map(decrypt_endpoint).collect()
}

/// A helper struct for the joined query result of OpenAIModel + OpenAIEndpoint
#[derive(Debug, Clone, sqlx::FromRow)]
struct ModelEndpointRow {
    // OpenAIModel fields
    pub id: i32,
    pub endpoint_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: bigdecimal::BigDecimal,
    pub output_token_price: bigdecimal::BigDecimal,
    pub description: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    // OpenAIEndpoint fields (aliased)
    pub ep_id: i32,
    pub ep_name: String,
    pub ep_api_base: String,
    pub ep_api_key: String,
    pub ep_has_responses_api: bool,
    pub ep_tags: Vec<String>,
    pub ep_proxies: Vec<String>,
    pub ep_status: String,
    pub ep_description: String,
    pub ep_created_at: chrono::NaiveDateTime,
    pub ep_updated_at: chrono::NaiveDateTime,
}

impl ModelEndpointRow {
    fn into_tuple(self) -> Result<(OpenAIModel, OpenAIEndpoint), sqlx::Error> {
        let model = OpenAIModel {
            id: self.id,
            endpoint_id: self.endpoint_id,
            model_id: self.model_id,
            has_image_generation: self.has_image_generation,
            has_speech: self.has_speech,
            has_chat_completion: self.has_chat_completion,
            has_embedding: self.has_embedding,
            input_token_price: self.input_token_price,
            output_token_price: self.output_token_price,
            description: self.description,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        let endpoint = OpenAIEndpoint {
            id: self.ep_id,
            name: self.ep_name,
            api_base: self.ep_api_base,
            api_key: self.ep_api_key,
            has_responses_api: self.ep_has_responses_api,
            tags: self.ep_tags,
            proxies: self.ep_proxies,
            status: self.ep_status,
            description: self.ep_description,
            created_at: self.ep_created_at,
            updated_at: self.ep_updated_at,
        };
        // Decrypt the api_key from the joined endpoint data
        let endpoint = decrypt_endpoint(endpoint)?;
        Ok((model, endpoint))
    }
}

/// Find all OpenAI models with a given model_id (name) that match the specified capacity options,
/// along with their associated endpoint information (api_key, api_base).
///
/// Only capacity fields set to `Some(true)` in `capacity` will be used as filters.
pub async fn find_models_by_name_and_capacity(
    pool: &DbPool,
    model_name: &str,
    capacity: &CapacityOption,
) -> Result<Vec<(OpenAIModel, OpenAIEndpoint)>, sqlx::Error> {
    // Build dynamic query with optional capacity filters
    let mut sql = String::from(
        "SELECT m.id, m.endpoint_id, m.model_id, m.has_image_generation, m.has_speech,
                m.has_chat_completion, m.has_embedding, m.input_token_price, m.output_token_price,
                m.description, m.created_at, m.updated_at,
                e.id AS ep_id, e.name AS ep_name, e.api_base AS ep_api_base,
                e.api_key AS ep_api_key, e.has_responses_api AS ep_has_responses_api,
                e.tags AS ep_tags, e.proxies AS ep_proxies,
                e.status AS ep_status, e.description AS ep_description,
                e.created_at AS ep_created_at, e.updated_at AS ep_updated_at
         FROM openai_models m
         INNER JOIN openai_endpoints e ON m.endpoint_id = e.id
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

    let mut query = sqlx::query_as::<_, ModelEndpointRow>(&sql).bind(model_name);

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
