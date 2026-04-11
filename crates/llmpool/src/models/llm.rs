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
    pub id: i64,
    pub name: String,
    pub api_base: String,
    pub encrypted_api_key: String,
    pub ellipsed_api_key: String,
    /// Decrypted API key, populated after reading from DB. Not stored in the database.
    #[sqlx(skip)]
    #[serde(skip)]
    pub api_key: String,

    pub provider: String,
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

fn default_max_tokens() -> i64 {
    100000
}

/// Represents a model available on an OpenAI-compatible upstream
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LLMModel {
    pub id: i64,
    pub upstream_id: i64,
    /// The full model identifier (e.g. "provider/model-name"), used when sending requests to upstream
    pub fullname: String,
    /// The short name after "/" in fullname; equals fullname if no "/" present. Used for client-facing model name matching.
    pub cname: String,
    pub is_active: bool,
    /// Features supported by this model.
    /// OpenAI examples: "chat/completions", "images", "embeddings", "audio/speech", "responses"
    /// Anthropic examples: "messages", "files", "messages/batches"
    pub features: Vec<String>,
    pub max_tokens: i64,
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
    pub upstream_id: i64,
    /// The full model identifier (e.g. "provider/model-name")
    pub fullname: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i64,
    pub input_token_price: BigDecimal,
    pub output_token_price: BigDecimal,
    pub batch_input_token_price: BigDecimal,
    pub batch_output_token_price: BigDecimal,
}

/// Used to update an existing OpenAI model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLLMModel {
    /// If provided, updates both fullname and cname (cname is derived from fullname)
    pub fullname: Option<String>,
    pub is_active: Option<bool>,
    /// If provided, replaces the entire features array
    pub features: Option<Vec<String>>,
    pub max_tokens: Option<i64>,
    pub input_token_price: Option<BigDecimal>,
    pub output_token_price: Option<BigDecimal>,
    pub batch_input_token_price: Option<BigDecimal>,
    pub batch_output_token_price: Option<BigDecimal>,
    pub description: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Used to update an existing OpenAI upstream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLLMUpstream {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub tags: Option<Vec<String>>,
    pub proxies: Option<Vec<String>>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}
