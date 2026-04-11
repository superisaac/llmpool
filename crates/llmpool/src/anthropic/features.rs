use reqwest::Client;

use crate::db::{self, DbPool};
use crate::models::UpdateLLMModel;
use chrono::Utc;

/// Anthropic feature identifiers
pub const FEATURE_MESSAGES: &str = "messages";
pub const FEATURE_FILES: &str = "files";
pub const FEATURE_MESSAGES_BATCHES: &str = "messages/batches";

/// Check whether the given upstream supports the Anthropic `/v1/messages` endpoint
/// by sending a minimal request and inspecting the response.
///
/// Returns `true` if the endpoint exists (even if the model/request is invalid),
/// and `false` only when the endpoint itself is absent (HTTP 404/405).
pub async fn check_messages_api_support(api_key: &str, api_base: &str) -> bool {
    let client = Client::new();
    let url = format!("{}/v1/messages", api_base.trim_end_matches('/'));

    // Send a minimal (intentionally invalid) request — we only care about whether
    // the path exists, not whether the request succeeds.
    let body = serde_json::json!({
        "model": "~nosuchmodel",
        "messages": [{"role": "user", "content": "a"}],
        "max_tokens": 1
    });

    match client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            // 404 / 405 → endpoint does not exist
            // Any other status (200, 400, 401, 529, …) → endpoint exists
            status != 404 && status != 405
        }
        Err(_) => false,
    }
}

/// Check whether the given upstream supports the Anthropic `/v1/files` endpoint.
pub async fn check_files_api_support(api_key: &str, api_base: &str) -> bool {
    let client = Client::new();
    let url = format!("{}/v1/files", api_base.trim_end_matches('/'));

    match client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            status != 404 && status != 405
        }
        Err(_) => false,
    }
}

/// Check whether the given upstream supports the Anthropic `/v1/messages/batches` endpoint.
pub async fn check_messages_batches_api_support(api_key: &str, api_base: &str) -> bool {
    let client = Client::new();
    let url = format!("{}/v1/messages/batches", api_base.trim_end_matches('/'));

    match client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            status != 404 && status != 405
        }
        Err(_) => false,
    }
}

/// Detect which Anthropic features are supported by the given upstream credentials.
/// Returns a Vec<String> of supported feature identifiers: "messages", "files", "messages/batches".
pub async fn detect_features(api_key: &str, api_base: &str) -> Vec<String> {
    let mut features = Vec::new();

    if check_messages_api_support(api_key, api_base).await {
        features.push(FEATURE_MESSAGES.to_string());
    }
    if check_files_api_support(api_key, api_base).await {
        features.push(FEATURE_FILES.to_string());
    }
    if check_messages_batches_api_support(api_key, api_base).await {
        features.push(FEATURE_MESSAGES_BATCHES.to_string());
    }

    features
}

/// Update the features of a model in the database using Anthropic feature detection.
/// Fetches the model and its upstream, runs detect_features, merges with existing
/// non-Anthropic features, and writes the result back to the database.
/// Only the features field is updated; all other fields remain unchanged.
pub async fn update_features(
    pool: &DbPool,
    model_pk: i64,
) -> Result<crate::models::LLMModel, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch the model record
    let model = db::llm::get_model(pool, model_pk).await?;

    // 2. Fetch the upstream to get api_key and api_base
    let upstream = db::llm::get_upstream(pool, model.upstream_id).await?;

    // 3. Detect Anthropic features
    let anthropic_features = detect_features(&upstream.api_key, &upstream.api_base).await;

    // 4. Merge with existing features: keep non-Anthropic features, replace Anthropic ones
    let anthropic_feature_set: std::collections::HashSet<&str> =
        [FEATURE_MESSAGES, FEATURE_FILES, FEATURE_MESSAGES_BATCHES]
            .iter()
            .copied()
            .collect();

    let mut merged_features: Vec<String> = model
        .features
        .iter()
        .filter(|f| !anthropic_feature_set.contains(f.as_str()))
        .cloned()
        .collect();
    merged_features.extend(anthropic_features);

    // 5. Update only the features field in the database
    let update = UpdateLLMModel {
        fullname: None,
        is_active: None,
        features: Some(merged_features),
        max_tokens: None,
        input_token_price: None,
        output_token_price: None,
        batch_input_token_price: None,
        batch_output_token_price: None,
        description: None,
        updated_at: Some(Utc::now().naive_utc()),
    };
    let updated = db::llm::update_model(pool, model_pk, &update).await?;
    Ok(updated)
}
