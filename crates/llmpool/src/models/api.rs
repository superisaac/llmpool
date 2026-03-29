use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// LLMAPIKey
// ============================================================

/// Represents an API access key associated with a user
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OpenAIAPIKey {
    pub id: i32,
    pub consumer_id: Option<i32>,
    pub apikey: String,
    pub label: String,
    pub is_active: bool,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOpenAIAPIKey {
    pub consumer_id: Option<i32>,
    pub apikey: String,
    pub label: String,
    pub expires_at: Option<NaiveDateTime>,
}
