use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// Fund
// ============================================================

/// Represents a user's fund record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Fund {
    pub id: i32,
    pub consumer_id: i32,
    pub cash: BigDecimal,
    pub credit: BigDecimal,
    pub debt: BigDecimal,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Fund {
    pub fn available(&self) -> BigDecimal {
        self.cash.clone() + self.credit.clone()
    }
}

/// Used to insert a new fund
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFund {
    pub consumer_id: i32,
    pub cash: BigDecimal,
    pub credit: BigDecimal,
    pub debt: BigDecimal,
}

/// Used to update an existing fund
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFund {
    pub cash: Option<BigDecimal>,
    pub credit: Option<BigDecimal>,
    pub debt: Option<BigDecimal>,
    pub updated_at: Option<NaiveDateTime>,
}

// ============================================================
// BalanceChange
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendToken {
    pub input_tokens: i64,
    pub input_token_price: BigDecimal,
    pub input_spend_amount: BigDecimal,
    pub output_tokens: i64,
    pub output_token_price: BigDecimal,
    pub output_spend_amount: BigDecimal,
    pub total_tokens: i64,
    pub event_id: i64,
}

/// The content of a balance change event, stored as JSON in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BalanceChangeContent {
    SpendToken(SpendToken),
    Deposit { amount: BigDecimal },
    Withdraw { amount: BigDecimal },
    Credit { amount: BigDecimal },
}

/// Represents a balance change record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct BalanceChange {
    pub id: i32,
    pub consumer_id: i32,
    pub unique_request_id: String,
    pub content: serde_json::Value,
    pub is_applied: bool,
    pub created_at: NaiveDateTime,
}

// impl BalanceChange {
//     /// Parse the content JSON into a BalanceChangeContent enum
//     pub fn parse_content(&self) -> Result<BalanceChangeContent, serde_json::Error> {
//         serde_json::from_value(self.content.clone())
//     }
// }

/// Used to insert a new balance change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBalanceChange {
    pub consumer_id: i32,
    pub unique_request_id: String,
    pub content: serde_json::Value,
}

impl NewBalanceChange {
    /// Create a new balance change from a BalanceChangeContent enum with a given unique_request_id
    pub fn from_content(
        consumer_id: i32,
        unique_request_id: String,
        content: &BalanceChangeContent,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            consumer_id,
            unique_request_id,
            content: serde_json::to_value(content)?,
        })
    }

    // /// Create a new balance change with an auto-generated UUIDv7 unique_request_id
    // pub fn from_content_auto_id(
    //     user_id: i32,
    //     content: &BalanceChangeContent,
    // ) -> Result<Self, serde_json::Error> {
    //     Ok(Self {
    //         user_id,
    //         unique_request_id: Uuid::now_v7().to_string(),
    //         content: serde_json::to_value(content)?,
    //     })
    // }
}
