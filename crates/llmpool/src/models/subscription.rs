use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// SubscriptionPlan
// ============================================================

/// Status values for a SubscriptionPlan
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionPlanStatus {
    #[sqlx(rename = "created")]
    Created,
    #[sqlx(rename = "started")]
    Started,
    #[sqlx(rename = "active")]
    Active,
    #[sqlx(rename = "canceled")]
    Canceled,
    #[sqlx(rename = "expired")]
    Expired,
}

/// Represents a subscription plan
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub id: i32,
    pub status: String,
    pub description: String,
    pub input_token_limit: i64,
    pub output_token_limit: i64,
    pub money_limit: BigDecimal,
    pub start_at: Option<NaiveDateTime>,
    pub end_at: Option<NaiveDateTime>,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// ============================================================
// Subscription
// ============================================================

/// Status values for a Subscription
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "varchar")]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    #[sqlx(rename = "deducted")]
    Deducted,
    #[sqlx(rename = "active")]
    Active,
    #[sqlx(rename = "filled")]
    Filled,
    #[sqlx(rename = "canceled")]
    Canceled,
}

/// Represents a user's subscription record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Subscription {
    pub id: i32,
    pub account_id: i32,
    pub plan_id: i32,
    pub status: String,
    pub used_input_tokens: i64,
    pub used_output_tokens: i64,
    pub used_money: BigDecimal,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
