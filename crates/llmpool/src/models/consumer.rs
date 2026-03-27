use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// Consumer
// ============================================================

/// Represents a consumer
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Consumer {
    pub id: i32,
    pub name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new consumer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewConsumer {
    pub name: String,
}

/// Used to update an existing consumer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConsumer {
    pub name: Option<String>,
    pub is_active: Option<bool>,
}
