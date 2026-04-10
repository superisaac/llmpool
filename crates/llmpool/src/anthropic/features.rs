use reqwest::Client;

use crate::db::{self, DbPool};
use crate::models::UpdateLLMModel;
use chrono::Utc;

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

/// Detect whether a model (identified by its database primary key) supports the
/// Anthropic `/v1/messages` API, update the `has_messages` flag in the database,
/// and return the updated `LLMModel`.
pub async fn detect_and_update_model_features(
    pool: &DbPool,
    model_pk: i64,
) -> Result<crate::models::LLMModel, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch the model record
    let model = db::llm::get_model(pool, model_pk).await?;

    // 2. Fetch the upstream to get api_key and api_base
    let upstream = db::llm::get_upstream(pool, model.upstream_id).await?;

    // 3. Detect has_messages
    let has_messages = check_messages_api_support(&upstream.api_key, &upstream.api_base).await;

    // 4. Update only has_messages in the database
    let update = UpdateLLMModel {
        fullname: None,
        is_active: None,
        has_image_generation: None,
        has_speech: None,
        has_chat_completion: None,
        has_embedding: None,
        has_messages: Some(has_messages),
        has_responses_api: None,
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
