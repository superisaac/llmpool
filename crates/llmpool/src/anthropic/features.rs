use anthropic_sdk::{Anthropic, MessageContent, MessageCreateParams, MessageParam, Role};

use crate::models::LLMModel;

/// Anthropic feature identifiers
pub const FEATURE_MESSAGES: &str = "messages";
pub const FEATURE_FILES: &str = "files";
pub const FEATURE_MESSAGES_BATCHES: &str = "messages/batches";

/// Check whether the given upstream supports the Anthropic `/v1/messages` endpoint
/// by sending a minimal request and inspecting the response.
///
/// Returns `true` if the endpoint exists (even if the model/request is invalid),
/// and `false` only when the endpoint itself is absent (HTTP 404/405).
pub async fn check_messages_api_support(client: &Anthropic, model: &LLMModel) -> bool {
    let params = MessageCreateParams {
        model: model.fullname.clone(),
        max_tokens: 1,
        messages: vec![MessageParam {
            role: Role::User,
            content: MessageContent::Text("ping".to_string()),
        }],
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stop_sequences: None,
        stream: Some(false),
        tools: None,
        tool_choice: None,
        metadata: None,
    };

    match client.messages().create(params).await {
        Ok(_resp) => true,
        Err(err) => !matches!(err.status_code(), Some(404 | 405)),
    }
}

/// Check whether the given upstream supports the Anthropic `/v1/files` endpoint.
pub async fn check_files_api_support(client: &Anthropic, _model: &LLMModel) -> bool {
    match client.files().list(None).await {
        Ok(_resp) => true,
        Err(err) => !matches!(err.status_code(), Some(404 | 405)),
    }
}

/// Check whether the given upstream supports the Anthropic `/v1/messages/batches` endpoint.
pub async fn check_messages_batches_api_support(client: &Anthropic, _model: &LLMModel) -> bool {
    match client.batches().list(None).await {
        Ok(_resp) => true,
        Err(err) => !matches!(err.status_code(), Some(404 | 405)),
    }
}

/// Detect which Anthropic features are supported by the given upstream credentials.
/// Returns a Vec<String> of supported feature identifiers: "messages", "files", "messages/batches".
pub async fn detect_features(client: &Anthropic, model: &LLMModel) -> Vec<String> {
    let mut features = Vec::new();

    if check_messages_api_support(client, model).await {
        features.push(FEATURE_MESSAGES.to_string());
    }
    if check_files_api_support(client, model).await {
        features.push(FEATURE_FILES.to_string());
    }
    if check_messages_batches_api_support(client, model).await {
        features.push(FEATURE_MESSAGES_BATCHES.to_string());
    }

    features
}
