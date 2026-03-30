use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents a session event entry
#[allow(dead_code)]
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: i64,
    pub session_id: String,
    pub session_index: i32,
    pub account_id: i32,
    pub model_id: i32,
    pub api_key_id: i32,
    pub input_token_price: BigDecimal,
    pub input_tokens: i64,
    pub output_token_price: BigDecimal,
    pub output_tokens: i64,
    pub event_data: serde_json::Value,
    pub created_at: NaiveDateTime,
}

/// Used to insert a new session event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionEvent {
    pub session_id: String,
    pub session_index: i32,
    pub account_id: i32,
    pub model_id: i32,
    pub api_key_id: i32,
    pub input_token_price: BigDecimal,
    pub input_tokens: i64,
    pub output_token_price: BigDecimal,
    pub output_tokens: i64,
    pub event_data: serde_json::Value,
}
