use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// Account
// ============================================================

/// Represents an account
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAccount {
    pub name: String,
}

/// Used to update an existing account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccount {
    pub name: Option<String>,
    pub is_active: Option<bool>,
}
