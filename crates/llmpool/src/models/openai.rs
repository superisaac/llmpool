use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Valid provider values for an LLM upstream
pub const VALID_PROVIDERS: &[&str] = &["openai", "azure", "cohere", "anthropic", "vllm", "ollama"];

fn default_provider() -> String {
    "openai".to_string()
}

/// Represents an OpenAI-compatible API upstream
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LLMUpstream {
    pub id: i32,
    pub name: String,
    pub api_base: String,
    pub encrypted_api_key: String,
    pub ellipsed_api_key: String,
    /// Decrypted API key, populated after reading from DB. Not stored in the database.
    #[sqlx(skip)]
    #[serde(skip)]
    pub api_key: String,

    pub provider: String,
    pub has_responses_api: bool,
    pub tags: Vec<String>,
    pub proxies: Vec<String>,
    pub status: String,
    pub description: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new OpenAI upstream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLLMUpstream {
    pub name: String,
    pub api_base: String,
    pub api_key: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    pub has_responses_api: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub proxies: Vec<String>,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub description: String,
}

fn default_status() -> String {
    "online".to_string()
}

/// Used to update an existing OpenAI upstream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLLMUpstream {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub has_responses_api: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub proxies: Option<Vec<String>>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Represents a model available on an OpenAI-compatible upstream
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LLMModel {
    pub id: i32,
    pub upstream_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: BigDecimal,
    pub output_token_price: BigDecimal,
    pub batch_input_token_price: BigDecimal,
    pub batch_output_token_price: BigDecimal,
    pub description: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new OpenAI model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLLMModel {
    pub upstream_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: BigDecimal,
    pub output_token_price: BigDecimal,
    pub batch_input_token_price: BigDecimal,
    pub batch_output_token_price: BigDecimal,
}

/// Options for filtering models by their capabilities.
/// Only fields set to `Some(true)` will be used as filters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapacityOption {
    pub has_chat_completion: Option<bool>,
    pub has_embedding: Option<bool>,
    pub has_image_generation: Option<bool>,
    pub has_speech: Option<bool>,
}

/// Used to update an existing OpenAI model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLLMModel {
    pub model_id: Option<String>,
    pub has_image_generation: Option<bool>,
    pub has_speech: Option<bool>,
    pub has_chat_completion: Option<bool>,
    pub has_embedding: Option<bool>,
    pub input_token_price: Option<BigDecimal>,
    pub output_token_price: Option<BigDecimal>,
    pub batch_input_token_price: Option<BigDecimal>,
    pub batch_output_token_price: Option<BigDecimal>,
    pub description: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}
