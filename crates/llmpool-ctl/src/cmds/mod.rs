pub mod apikey;
pub mod consumer;
pub mod endpoint;
pub mod fund;
pub mod model;
pub mod session_event;

use serde::Deserialize;

// Re-export subcommand enums and handlers
pub use apikey::ApiKeyAction;
pub use consumer::ConsumerAction;
pub use endpoint::EndpointAction;
pub use fund::FundAction;
pub use model::ModelAction;
pub use session_event::SessionEventAction;

pub use apikey::handle_apikey;
pub use consumer::handle_consumer;
pub use endpoint::handle_endpoint;
pub use fund::handle_fund;
pub use model::handle_model;
pub use session_event::handle_session_event;

// ============================================================
// Common API Response Types
// ============================================================

#[derive(Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Deserialize)]
pub struct PaginationInfo {
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
}

#[derive(Debug, Deserialize)]
pub struct CursorResponse<T> {
    pub data: Vec<T>,
    pub next_id: i64,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct EndpointResponse {
    pub id: i32,
    pub name: String,
    pub api_base: String,
    pub has_responses_api: bool,
    pub tags: Vec<String>,
    pub proxies: Vec<String>,
    pub status: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ModelResponse {
    pub id: i32,
    pub endpoint_id: i32,
    pub model_id: String,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub input_token_price: String,
    pub output_token_price: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsumerResponse {
    pub id: i32,
    pub name: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct FundResponse {
    pub id: i32,
    pub consumer_id: i32,
    pub cash: String,
    pub credit: String,
    pub debt: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct BalanceChangeResponse {
    pub id: i32,
    pub consumer_id: i32,
    pub unique_request_id: String,
    pub content: serde_json::Value,
    pub is_applied: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct EndpointWithModelsResponse {
    pub endpoint: EndpointResponse,
    pub models: Vec<ModelResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ModelFeaturesResponse {
    pub model_id: String,
    pub owned_by: String,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub has_image_generation: bool,
    pub has_speech: bool,
}

#[derive(Debug, Deserialize)]
pub struct TestEndpointResponse {
    pub has_responses_api: bool,
    pub models: Vec<ModelFeaturesResponse>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TagsResponse {
    pub endpoint_id: i32,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OpenAIAPIKeyResponse {
    pub id: i32,
    pub consumer_id: Option<i32>,
    pub apikey: String,
    pub label: String,
    pub is_active: bool,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ============================================================
// Common Display Helpers
// ============================================================

pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}

pub fn bool_mark(v: bool) -> &'static str {
    if v { "✓" } else { "✗" }
}

pub fn parse_comma_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

pub fn print_pagination(pagination: &PaginationInfo) {
    if pagination.total_pages > 1 {
        println!(
            "\nPage {}/{} (total: {}, page_size: {})",
            pagination.page, pagination.total_pages, pagination.total, pagination.page_size
        );
    } else {
        println!("\nTotal: {}", pagination.total);
    }
}

pub fn print_models(models: &[ModelResponse]) {
    if models.is_empty() {
        println!("No models found.");
        return;
    }

    println!(
        "{:<5} {:<10} {:<35} {:<6} {:<6} {:<6} {:<6} {:<14} {:<14} {:<20}",
        "ID",
        "EP ID",
        "Model ID",
        "Chat",
        "Embed",
        "Image",
        "Speech",
        "Input Price",
        "Output Price",
        "Description"
    );
    println!("{}", "-".repeat(128));
    for m in models {
        println!(
            "{:<5} {:<10} {:<35} {:<6} {:<6} {:<6} {:<6} {:<14} {:<14} {:<20}",
            m.id,
            m.endpoint_id,
            truncate(&m.model_id, 33),
            bool_mark(m.has_chat_completion),
            bool_mark(m.has_embedding),
            bool_mark(m.has_image_generation),
            bool_mark(m.has_speech),
            truncate(&m.input_token_price, 12),
            truncate(&m.output_token_price, 12),
            truncate(&m.description, 18),
        );
    }
}

// ============================================================
// Common ID Resolution Helpers
// ============================================================

/// Resolve an endpoint name or ID string to a numeric endpoint ID.
pub async fn resolve_endpoint_id(
    endpoint: &str,
    client: &crate::client::ApiClient,
) -> Result<i32, String> {
    if let Ok(id) = endpoint.parse::<i32>() {
        return Ok(id);
    }
    let resp: EndpointResponse = client
        .get(&format!("/endpoint_by_name/{}", endpoint))
        .await?;
    Ok(resp.id)
}

/// Resolve a consumer name or consumer ID string to a numeric consumer ID.
pub async fn resolve_consumer_id(
    consumer: &str,
    client: &crate::client::ApiClient,
) -> Result<i32, String> {
    if let Ok(id) = consumer.parse::<i32>() {
        return Ok(id);
    }
    let resp: ConsumerResponse = client
        .get(&format!("/consumers_by_name/{}", consumer))
        .await?;
    Ok(resp.id)
}
