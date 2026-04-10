//! Anthropic API proxy types
//!
//! Provides typed request/response structs used by the Anthropic proxy handlers.
//! All upstream HTTP calls are made via `anthropic-sdk-rust` (the `Anthropic` client)
//! or the generic helpers in `super::helpers`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Shared / common types
// ---------------------------------------------------------------------------

/// A single content block in a message (text, image, tool_use, tool_result, …)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlock {
    /// Simple string shorthand
    Text(String),
    /// Structured content block (image, tool_use, tool_result, …)
    Block(Value),
}

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageParam {
    pub role: String,
    pub content: ContentBlock,
}

/// Usage statistics returned by the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// POST /v1/messages — Create a Message
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/messages`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageParams {
    /// The model to use (e.g. "claude-3-5-sonnet-20241022")
    pub model: String,
    /// The conversation messages
    pub messages: Vec<MessageParam>,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Optional system prompt (string or array of content blocks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Sampling temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Top-p nucleus sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Top-k sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Tool definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    /// Tool choice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Response body for `POST /v1/messages`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub content: Vec<Value>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

// ---------------------------------------------------------------------------
// POST /v1/messages/batches — Create a Message Batch
// ---------------------------------------------------------------------------

/// A single request item in a message batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequestItem {
    /// A developer-supplied custom ID for this request
    pub custom_id: String,
    /// The message creation parameters for this item
    pub params: CreateMessageParams,
}

/// Request body for `POST /v1/messages/batches`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageBatchParams {
    /// The list of requests to process
    pub requests: Vec<BatchRequestItem>,
}

/// Processing status counts for a message batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequestCounts {
    pub processing: u64,
    pub succeeded: u64,
    pub errored: u64,
    pub canceled: u64,
    pub expired: u64,
}

/// Response body for batch create / retrieve / cancel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBatch {
    pub id: String,
    #[serde(rename = "type")]
    pub batch_type: String,
    pub processing_status: String,
    pub request_counts: BatchRequestCounts,
    pub ended_at: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub cancel_initiated_at: Option<String>,
    pub results_url: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches — List Message Batches
// ---------------------------------------------------------------------------

/// Query parameters for listing message batches
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListMessageBatchesParams {
    /// Cursor: return results before this ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    /// Cursor: return results after this ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    /// Number of items per page (1–100, default 20)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Paginated list response for message batches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMessageBatchesResponse {
    pub data: Vec<MessageBatch>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

// ---------------------------------------------------------------------------
// GET /v1/messages/batches/{id}/results — Retrieve Message Batch results
// ---------------------------------------------------------------------------

/// The result of a single request in a message batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBatchIndividualResponse {
    pub custom_id: String,
    pub result: Value,
}

// ---------------------------------------------------------------------------
// POST /v1/messages/count_tokens — Count tokens in a Message
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/messages/count_tokens`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountMessageTokensParams {
    /// The model to use
    pub model: String,
    /// The conversation messages
    pub messages: Vec<MessageParam>,
    /// Optional system prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    /// Tool definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    /// Tool choice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

/// Response body for `POST /v1/messages/count_tokens`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountMessageTokensResponse {
    pub input_tokens: u64,
}
