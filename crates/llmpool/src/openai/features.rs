use async_openai::{
    Client,
    error::OpenAIError,
    types::{
        audio::{CreateSpeechRequestArgs, SpeechModel, SpeechResponseFormat, Voice::Alloy},
        chat::{
            ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
            ChatCompletionRequestUserMessageContent, CreateChatCompletionRequestArgs,
        },
        embeddings::{CreateEmbeddingRequestArgs, EmbeddingInput},
        images::{CreateImageRequestArgs, ImageModel},
        models::Model,
    },
};
use chrono::Utc;

use bigdecimal::BigDecimal;
use std::str::FromStr;

use crate::db::{self, DbPool};
use crate::models::{NewLLMModel, NewLLMUpstream, UpdateLLMUpstream};

/// OpenAI feature identifiers
pub const FEATURE_CHAT_COMPLETIONS: &str = "chat/completions";
pub const FEATURE_IMAGES: &str = "images";
pub const FEATURE_EMBEDDINGS: &str = "embeddings";
pub const FEATURE_AUDIO_SPEECH: &str = "audio/speech";
pub const FEATURE_RESPONSES: &str = "responses";

/// Detect model features through actual upstream calls.
/// Returns a Vec<String> of supported feature identifiers.
pub async fn detect_features(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> Vec<String> {
    let mut features = Vec::new();

    if check_chat_completion_support(client, model).await {
        features.push(FEATURE_CHAT_COMPLETIONS.to_string());
    }
    if check_image_generation_support(client, model).await {
        features.push(FEATURE_IMAGES.to_string());
    }
    if check_embedding_support(client, model).await {
        features.push(FEATURE_EMBEDDINGS.to_string());
    }
    if check_speech_support(client, model).await {
        features.push(FEATURE_AUDIO_SPEECH.to_string());
    }
    if check_responses_api_support(client, model).await {
        features.push(FEATURE_RESPONSES.to_string());
    }

    features
}

async fn check_image_generation_support(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> bool {
    let model = ImageModel::Other(model.clone().id);
    let request = CreateImageRequestArgs::default()
        .model(model)
        .prompt("a")
        .n(1)
        .build();

    if let Err(_) = request {
        return false;
    }

    match client.images().generate(request.unwrap()).await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

async fn check_speech_support(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> bool {
    let speech_model = SpeechModel::Other(model.clone().id);
    let request = CreateSpeechRequestArgs::default()
        .model(speech_model)
        .input("a")
        .voice(Alloy)
        .response_format(SpeechResponseFormat::Mp3)
        .build();

    if let Err(_) = request {
        return false;
    }

    match client.audio().speech().create(request.unwrap()).await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

/// Check if the model supports chat completion by attempting to create a chat completion
async fn check_chat_completion_support(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> bool {
    let user_message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
        content: ChatCompletionRequestUserMessageContent::Text("a".to_string()),
        name: None,
    });

    let request = CreateChatCompletionRequestArgs::default()
        .model(model.id.clone())
        .messages(vec![user_message])
        .max_completion_tokens(1u32)
        .build();

    if let Err(_) = request {
        return false;
    }

    match client.chat().create(request.unwrap()).await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

/// Check if the model supports embedding by attempting to create an embedding
async fn check_embedding_support(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> bool {
    let request = CreateEmbeddingRequestArgs::default()
        .model(model.id.clone())
        .input(EmbeddingInput::String("a".to_string()))
        .build();

    if let Err(_) = request {
        return false;
    }

    match client.embeddings().create(request.unwrap()).await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

/// Check if the model supports the OpenAI /v1/responses API
async fn check_responses_api_support(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> bool {
    use async_openai::types::responses::{CreateResponse, InputParam};

    let payload = CreateResponse {
        model: Some(model.id.clone()),
        input: InputParam::Text("a".to_string()),
        max_output_tokens: Some(1),
        ..Default::default()
    };

    match client.responses().create(payload).await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

/// Fetch the model list from the upstream and save each model to the database without
/// performing any feature detection. All features are set to an empty array.
/// This will upsert the upstream (by api_base) and insert any new models (by upstream_id + model_id).
pub async fn list_and_save_without_detect(
    pool: &DbPool,
    name: &str,
    api_key: &str,
    api_base: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch the model list from the remote API (no feature probing)
    let config = async_openai::config::OpenAIConfig::new()
        .with_api_key(api_key.to_string())
        .with_api_base(api_base.to_string());
    let client = Client::with_config(config);

    let response = client.models().list().await?;
    let mut models = response.data;
    models.sort_by(|a, b| a.id.cmp(&b.id));

    // 2. Upsert the LLMUpstream
    let upstream = match db::llm::get_upstream_by_api_base(pool, api_base).await {
        Ok(existing) => {
            let update = UpdateLLMUpstream {
                name: Some(name.to_string()),
                api_base: None,
                api_key: Some(api_key.to_string()),
                provider: None,
                tags: None,
                proxies: None,
                status: None,
                description: None,
                updated_at: Some(Utc::now().naive_utc()),
            };
            db::llm::update_upstream(pool, existing.id, &update).await?
        }
        Err(sqlx::Error::RowNotFound) => {
            let new_upstream = NewLLMUpstream {
                name: name.to_string(),
                api_base: api_base.to_string(),
                api_key: api_key.to_string(),
                provider: "openai".to_string(),
                tags: vec![],
                proxies: vec![],
                status: "online".to_string(),
                description: String::new(),
            };
            db::llm::create_upstream(pool, &new_upstream).await?
        }
        Err(e) => return Err(Box::new(e)),
    };

    // 3. For each model, insert a record with empty features (skip if already exists)
    let default_token_price = BigDecimal::from_str("0.000001").unwrap();
    for model in &models {
        match db::llm::find_model_by_upstream_and_model_id(pool, upstream.id, &model.id).await {
            Ok(_existing) => {
                // Model already exists — leave it untouched so existing features are preserved
            }
            Err(sqlx::Error::RowNotFound) => {
                let new_model = NewLLMModel {
                    upstream_id: upstream.id,
                    fullname: model.id.clone(),
                    features: vec![],
                    max_tokens: 100000,
                    input_token_price: default_token_price.clone(),
                    output_token_price: default_token_price.clone(),
                    batch_input_token_price: default_token_price.clone(),
                    batch_output_token_price: default_token_price.clone(),
                };
                db::llm::create_model(pool, &new_model).await?;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }

    Ok(())
}

/// Helper function: Determine if error indicates feature is truly unavailable
fn is_unsupported_error(e: &OpenAIError) -> bool {
    let err_str = e.to_string().to_lowercase();
    // 404: Path does not exist or model does not exist under that path
    // 405: Method not allowed
    err_str.contains("404")
        || err_str.contains("not found")
        || err_str.contains("405")
        || err_str.contains("unsupported_model")
}
