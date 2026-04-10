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
    #[sqlx(rename = "active")]
    Active,
    #[sqlx(rename = "deactive")]
    Deactive,
}

/// Represents a subscription plan
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub id: i64,
    pub status: String,
    pub description: String,
    pub total_token_limit: i64,
    pub time_span: i64,
    pub money_limit: BigDecimal,
    pub sort_order: i64,
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
    #[sqlx(rename = "pending")]
    Pending,
    #[sqlx(rename = "active")]
    Active,
    #[sqlx(rename = "deactive")]
    Deactive,
}

/// Represents a user's subscription record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Subscription {
    pub id: i64,
    pub account_id: i64,
    pub plan_id: i64,
    pub status: String,
    pub start_at: Option<NaiveDateTime>,
    pub end_at: Option<NaiveDateTime>,
    pub used_total_tokens: i64,
    pub total_token_limit: i64,
    pub sort_order: i64,
    pub used_money: BigDecimal,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
