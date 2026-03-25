use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Represents a session event entry
#[allow(dead_code)]
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: i64,
    pub session_id: String,
    pub user_id: i32,
    pub model_id: i32,
    pub event_data: serde_json::Value,
    pub created_at: NaiveDateTime,
}

/// Used to insert a new session event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionEvent {
    pub session_id: String,
    pub user_id: i32,
    pub model_id: i32,
    pub event_data: serde_json::Value,
}
