use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents an OpenAI-compatible API endpoint
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OpenAIEndpoint {
    pub id: i32,
    pub name: String,
    pub api_base: String,
    pub api_key: String,
    pub has_responses_api: bool,
    pub tags: Vec<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new OpenAI endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOpenAIEndpoint {
    pub name: String,
    pub api_base: String,
    pub api_key: String,
    pub has_responses_api: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Used to update an existing OpenAI endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOpenAIEndpoint {
    pub name: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub has_responses_api: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Represents a model available on an OpenAI-compatible endpoint
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OpenAIModel {
    pub id: i32,
    pub endpoint_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: BigDecimal,
    pub output_token_price: BigDecimal,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new OpenAI model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOpenAIModel {
    pub endpoint_id: i32,
    pub model_id: String,
    pub has_image_generation: bool,
    pub has_speech: bool,
    pub has_chat_completion: bool,
    pub has_embedding: bool,
    pub input_token_price: BigDecimal,
    pub output_token_price: BigDecimal,
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
pub struct UpdateOpenAIModel {
    pub model_id: Option<String>,
    pub has_image_generation: Option<bool>,
    pub has_speech: Option<bool>,
    pub has_chat_completion: Option<bool>,
    pub has_embedding: Option<bool>,
    pub input_token_price: Option<BigDecimal>,
    pub output_token_price: Option<BigDecimal>,
    pub updated_at: Option<NaiveDateTime>,
}
