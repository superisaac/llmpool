//! Anthropic API client generated from docs/anthropic-spec.json
//!
//! Provides typed request/response structs and an `AnthropicApiClient` that
//! wraps a `reqwest::Client` and authenticates every request with the
//! `x-api-key` header.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Default Anthropic API version
// ---------------------------------------------------------------------------

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";

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
// POST /v1/complete — Create a Text Completion (legacy)
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/complete`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// The model to use
    pub model: String,
    /// The prompt to complete (must follow Human/Assistant format)
    pub prompt: String,
    /// Maximum tokens to generate
    pub max_tokens_to_sample: u32,
    /// Stop sequences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Sampling temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Top-p nucleus sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Top-k sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Response body for `POST /v1/complete`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    #[serde(rename = "type")]
    pub completion_type: String,
    pub id: String,
    pub completion: String,
    pub stop_reason: Option<String>,
    pub model: String,
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

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// An error returned by the Anthropic API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

/// Top-level error response envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(rename = "type")]
    pub response_type: String,
    pub error: ApiErrorDetail,
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors that can occur when calling the Anthropic API
#[derive(Debug)]
pub enum AnthropicApiError {
    /// A reqwest / network-level error
    Network(reqwest::Error),
    /// The API returned a non-2xx HTTP status
    Api { status: u16, body: String },
    /// JSON (de)serialization error
    Json(serde_json::Error),
}

impl std::fmt::Display for AnthropicApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnthropicApiError::Network(e) => write!(f, "network error: {e}"),
            AnthropicApiError::Api { status, body } => {
                write!(f, "API error (HTTP {status}): {body}")
            }
            AnthropicApiError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for AnthropicApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AnthropicApiError::Network(e) => Some(e),
            AnthropicApiError::Json(e) => Some(e),
            AnthropicApiError::Api { .. } => None,
        }
    }
}

impl From<reqwest::Error> for AnthropicApiError {
    fn from(e: reqwest::Error) -> Self {
        AnthropicApiError::Network(e)
    }
}

impl From<serde_json::Error> for AnthropicApiError {
    fn from(e: serde_json::Error) -> Self {
        AnthropicApiError::Json(e)
    }
}

// ---------------------------------------------------------------------------
// AnthropicApiClient
// ---------------------------------------------------------------------------

/// A client for the Anthropic REST API.
///
/// Every request is authenticated with the `x-api-key` header and includes
/// the `anthropic-version` header.
///
/// # Example
/// ```rust,no_run
/// use crate::views::anthropic_proxy::anthropic_api::AnthropicApiClient;
///
/// let client = AnthropicApiClient::new("sk-ant-…".to_string());
/// ```
pub struct AnthropicApiClient {
    http_client: reqwest::Client,
    api_key: String,
    api_base: String,
    anthropic_version: String,
}

impl AnthropicApiClient {
    /// Create a new client that talks to the default Anthropic API base URL.
    pub fn new(api_key: String) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            api_key,
            api_base: ANTHROPIC_API_BASE.to_string(),
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    /// Create a new client with a custom API base URL (useful for proxies /
    /// self-hosted deployments).
    pub fn with_base_url(api_key: String, api_base: String) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            api_key,
            api_base: api_base.trim_end_matches('/').to_string(),
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    /// Create a new client using an existing `reqwest::Client` (e.g. one
    /// configured with a proxy).
    pub fn with_http_client(
        http_client: reqwest::Client,
        api_key: String,
        api_base: String,
    ) -> Self {
        Self {
            http_client,
            api_key,
            api_base: api_base.trim_end_matches('/').to_string(),
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    /// Override the `anthropic-version` header value (default: `"2023-06-01"`).
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.anthropic_version = version.into();
        self
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Attach the common authentication / versioning headers to a request
    /// builder.
    fn auth_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.anthropic_version)
            .header("content-type", "application/json")
    }

    /// Consume a `reqwest::Response`, returning the body text on non-2xx
    /// status codes as an `Err(AnthropicApiError::Api { … })`.
    async fn check_response(
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, AnthropicApiError> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let status_u16 = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(AnthropicApiError::Api {
                status: status_u16,
                body,
            })
        }
    }

    // -----------------------------------------------------------------------
    // POST /v1/messages
    // -----------------------------------------------------------------------

    /// Create a Message.
    ///
    /// Send a structured list of input messages with text and/or image content,
    /// and the model will generate the next message in the conversation.
    ///
    /// For streaming responses use [`create_message_raw`] and handle the SSE
    /// byte stream yourself.
    pub async fn create_message(
        &self,
        params: &CreateMessageParams,
    ) -> Result<Message, AnthropicApiError> {
        let url = format!("{}/v1/messages", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<Message>().await?)
    }

    /// Create a Message and return the raw `reqwest::Response`.
    ///
    /// Useful when `stream: true` is set in `params` — the caller can then
    /// consume the SSE byte stream directly.
    pub async fn create_message_raw(
        &self,
        params: &CreateMessageParams,
    ) -> Result<reqwest::Response, AnthropicApiError> {
        let url = format!("{}/v1/messages", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        Self::check_response(resp).await
    }

    // -----------------------------------------------------------------------
    // POST /v1/complete  (legacy Text Completions)
    // -----------------------------------------------------------------------

    /// [Legacy] Create a Text Completion.
    ///
    /// The Text Completions API is a legacy API. Prefer the Messages API for
    /// new integrations.
    pub async fn create_completion(
        &self,
        params: &CompletionRequest,
    ) -> Result<CompletionResponse, AnthropicApiError> {
        let url = format!("{}/v1/complete", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<CompletionResponse>().await?)
    }

    /// [Legacy] Create a Text Completion and return the raw `reqwest::Response`.
    ///
    /// Useful when `stream: true` is set in `params`.
    pub async fn create_completion_raw(
        &self,
        params: &CompletionRequest,
    ) -> Result<reqwest::Response, AnthropicApiError> {
        let url = format!("{}/v1/complete", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        Self::check_response(resp).await
    }

    // -----------------------------------------------------------------------
    // POST /v1/messages/batches
    // -----------------------------------------------------------------------

    /// Create a Message Batch.
    ///
    /// Send a batch of Message creation requests. Once created, the batch
    /// begins processing immediately and can take up to 24 hours to complete.
    pub async fn create_message_batch(
        &self,
        params: &CreateMessageBatchParams,
    ) -> Result<MessageBatch, AnthropicApiError> {
        let url = format!("{}/v1/messages/batches", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<MessageBatch>().await?)
    }

    // -----------------------------------------------------------------------
    // GET /v1/messages/batches
    // -----------------------------------------------------------------------

    /// List all Message Batches within a Workspace.
    ///
    /// Most recently created batches are returned first.
    pub async fn list_message_batches(
        &self,
        params: &ListMessageBatchesParams,
    ) -> Result<ListMessageBatchesResponse, AnthropicApiError> {
        let url = format!("{}/v1/messages/batches", self.api_base);
        let mut builder = self.http_client.get(&url);

        // Append optional query parameters
        if let Some(ref before_id) = params.before_id {
            builder = builder.query(&[("before_id", before_id.as_str())]);
        }
        if let Some(ref after_id) = params.after_id {
            builder = builder.query(&[("after_id", after_id.as_str())]);
        }
        if let Some(limit) = params.limit {
            builder = builder.query(&[("limit", limit.to_string().as_str())]);
        }

        let resp = self.auth_headers(builder).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<ListMessageBatchesResponse>().await?)
    }

    // -----------------------------------------------------------------------
    // GET /v1/messages/batches/{message_batch_id}
    // -----------------------------------------------------------------------

    /// Retrieve a Message Batch.
    ///
    /// This endpoint is idempotent and can be used to poll for Message Batch
    /// completion. To access the results, use
    /// [`retrieve_message_batch_results`].
    pub async fn retrieve_message_batch(
        &self,
        message_batch_id: &str,
    ) -> Result<MessageBatch, AnthropicApiError> {
        let url = format!("{}/v1/messages/batches/{}", self.api_base, message_batch_id);
        let builder = self.http_client.get(&url);
        let resp = self.auth_headers(builder).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<MessageBatch>().await?)
    }

    // -----------------------------------------------------------------------
    // POST /v1/messages/batches/{message_batch_id}/cancel
    // -----------------------------------------------------------------------

    /// Cancel a Message Batch.
    ///
    /// Batches may be canceled any time before processing ends. Once
    /// cancellation is initiated, the batch enters a `canceling` state.
    pub async fn cancel_message_batch(
        &self,
        message_batch_id: &str,
    ) -> Result<MessageBatch, AnthropicApiError> {
        let url = format!(
            "{}/v1/messages/batches/{}/cancel",
            self.api_base, message_batch_id
        );
        let builder = self.http_client.post(&url);
        let resp = self
            .auth_headers(builder)
            .header("content-length", "0")
            .send()
            .await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<MessageBatch>().await?)
    }

    // -----------------------------------------------------------------------
    // GET /v1/messages/batches/{message_batch_id}/results
    // -----------------------------------------------------------------------

    /// Retrieve Message Batch results as a raw response.
    ///
    /// The response body is a `.jsonl` stream where each line is a
    /// [`MessageBatchIndividualResponse`] JSON object. Results are **not**
    /// guaranteed to be in the same order as the original requests — use the
    /// `custom_id` field to match them.
    pub async fn retrieve_message_batch_results_raw(
        &self,
        message_batch_id: &str,
    ) -> Result<reqwest::Response, AnthropicApiError> {
        let url = format!(
            "{}/v1/messages/batches/{}/results",
            self.api_base, message_batch_id
        );
        let builder = self.http_client.get(&url);
        let resp = self.auth_headers(builder).send().await?;
        Self::check_response(resp).await
    }

    /// Retrieve Message Batch results, collecting all lines into a `Vec`.
    ///
    /// For large batches prefer [`retrieve_message_batch_results_raw`] to
    /// stream the `.jsonl` response incrementally.
    pub async fn retrieve_message_batch_results(
        &self,
        message_batch_id: &str,
    ) -> Result<Vec<MessageBatchIndividualResponse>, AnthropicApiError> {
        let resp = self
            .retrieve_message_batch_results_raw(message_batch_id)
            .await?;
        let text = resp.text().await?;
        let mut results = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let item: MessageBatchIndividualResponse =
                serde_json::from_str(line).map_err(AnthropicApiError::Json)?;
            results.push(item);
        }
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // POST /v1/messages/count_tokens
    // -----------------------------------------------------------------------

    /// Count the number of tokens in a Message without creating it.
    pub async fn count_message_tokens(
        &self,
        params: &CountMessageTokensParams,
    ) -> Result<CountMessageTokensResponse, AnthropicApiError> {
        let url = format!("{}/v1/messages/count_tokens", self.api_base);
        let builder = self.http_client.post(&url);
        let resp = self.auth_headers(builder).json(params).send().await?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.json::<CountMessageTokensResponse>().await?)
    }
}
