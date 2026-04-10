pub mod account;
pub mod apikey;
pub mod model;
pub mod session_event;
pub mod subscription;
pub mod upstream;
pub mod wallet;

use serde::Deserialize;

// Re-export subcommand enums and handlers
pub use account::AccountAction;
pub use apikey::ApiKeyAction;
pub use model::ModelAction;
pub use session_event::SessionEventAction;
pub use subscription::{SubscriptionAction, SubscriptionPlanAction};
pub use upstream::UpstreamAction;
pub use wallet::WalletAction;

pub use account::handle_account;
pub use apikey::handle_apikey;
pub use model::handle_model;
pub use session_event::handle_session_event;
pub use subscription::{handle_subscription, handle_subscription_plan};
pub use upstream::handle_upstream;
pub use wallet::handle_wallet;

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
pub struct UpstreamResponse {
    pub id: i64,
    pub name: String,
    pub api_base: String,
    pub provider: String,
    pub tags: Vec<String>,
    pub proxies: Vec<String>,
    pub status: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ModelResponse {
    pub id: i64,
    pub upstream_id: i64,
    pub fullname: String,
    pub cname: String,
    pub is_active: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_responses_api: bool,
    pub input_token_price: String,
    pub output_token_price: String,
    pub batch_input_token_price: String,
    pub batch_output_token_price: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ModelTestResult {
    pub model_id: i64,
    pub model: Option<ModelResponse>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountResponse {
    pub id: i64,
    pub name: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WalletResponse {
    pub id: i64,
    pub account_id: i64,
    pub balance: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct BalanceChangeResponse {
    pub id: i64,
    pub account_id: i64,
    pub unique_request_id: String,
    pub content: serde_json::Value,
    pub is_applied: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamWithModelsResponse {
    pub upstream: UpstreamResponse,
    pub models: Vec<ModelResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ModelFeaturesResponse {
    pub fullname: String,
    pub owned_by: String,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub has_image_generation: bool,
    pub has_speech: bool,
}

#[derive(Debug, Deserialize)]
pub struct TestUpstreamResponse {
    pub models: Vec<ModelFeaturesResponse>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TagsResponse {
    pub upstream_id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ApiCredentialResponse {
    pub id: i64,
    pub account_id: Option<i64>,
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
            m.upstream_id,
            truncate(&m.fullname, 33),
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

/// Resolve an upstream name or ID string to a numeric upstream ID.
pub async fn resolve_upstream_id(
    upstream: &str,
    client: &crate::client::ApiClient,
) -> Result<i64, String> {
    if let Ok(id) = upstream.parse::<i64>() {
        return Ok(id);
    }
    let resp: UpstreamResponse = client
        .get(&format!("/upstream_by_name/{}", upstream))
        .await?;
    Ok(resp.id)
}

/// Resolve an account name or account ID string to a numeric account ID.
pub async fn resolve_account_id(
    account: &str,
    client: &crate::client::ApiClient,
) -> Result<i64, String> {
    if let Ok(id) = account.parse::<i64>() {
        return Ok(id);
    }
    let resp: AccountResponse = client
        .get(&format!("/accounts_by_name/{}", account))
        .await?;
    Ok(resp.id)
}
