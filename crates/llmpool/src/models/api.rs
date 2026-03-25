use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// AccessKey
// ============================================================

/// Represents an API access key associated with a user
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AccessKey {
    pub id: i32,
    pub user_id: Option<i32>,
    pub apikey: String,
    pub is_active: bool,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Used to insert a new access key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAccessKey {
    pub user_id: Option<i32>,
    pub apikey: String,
    pub expires_at: Option<NaiveDateTime>,
}

// /// Used to update an existing access key
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct UpdateAccessKey {
//     pub apikey: Option<String>,
//     pub is_active: Option<bool>,
//     pub expires_at: Option<Option<NaiveDateTime>>,
//     pub updated_at: Option<NaiveDateTime>,
// }
