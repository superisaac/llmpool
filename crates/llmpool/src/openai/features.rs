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
use crate::models::{NewLLMModel, NewLLMUpstream, UpdateLLMModel, UpdateLLMUpstream};

pub struct ModelFeatures {
    pub model: Model,
    pub has_image_generation: bool,
    // has_video: bool,
    pub has_speech: bool,
    pub has_embedding: bool,
    pub has_chat_completion: bool,
    pub has_responses_api: bool,
}

/// Detect model features through actual upstream calls
async fn detect_model_features(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> ModelFeatures {
    let has_image_generation = check_image_generation_support(client, model).await;
    let has_speech = check_speech_support(client, model).await;
    let has_embedding = check_embedding_support(client, model).await;
    let has_chat_completion = check_chat_completion_support(client, model).await;
    let has_responses_api = check_responses_api_support(client, model).await;

    ModelFeatures {
        model: model.clone(),
        has_image_generation,
        has_speech,
        has_embedding,
        has_chat_completion,
        has_responses_api,
    }
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
/// performing any feature detection. All feature flags are set to `false`.
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

    // 3. For each model, insert a record with all features set to false (skip if already exists)
    let default_token_price = BigDecimal::from_str("0.000001").unwrap();
    for model in &models {
        match db::llm::find_model_by_upstream_and_model_id(pool, upstream.id, &model.id).await {
            Ok(_existing) => {
                // Model already exists — leave it untouched so existing feature flags are preserved
            }
            Err(sqlx::Error::RowNotFound) => {
                let new_model = NewLLMModel {
                    upstream_id: upstream.id,
                    model_id: model.id.clone(),
                    has_image_generation: false,
                    has_speech: false,
                    has_chat_completion: false,
                    has_embedding: false,
                    has_messages: false,
                    has_responses_api: false,
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

/// Detect features for a single model given its upstream credentials, and update the database.
/// Only the feature flags (has_image_generation, has_speech, has_chat_completion, has_embedding)
/// are updated; all other fields remain unchanged.
pub async fn detect_and_update_model_features(
    pool: &DbPool,
    model_pk: i32,
) -> Result<crate::models::LLMModel, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch the model record
    let model = db::llm::get_model(pool, model_pk).await?;

    // 2. Fetch the upstream to get api_key and api_base
    let upstream = db::llm::get_upstream(pool, model.upstream_id).await?;

    // 3. Build the OpenAI client
    let config = async_openai::config::OpenAIConfig::new()
        .with_api_key(upstream.api_key.clone())
        .with_api_base(upstream.api_base.clone());
    let client = Client::with_config(config);

    // 4. Build a minimal Model struct for feature detection
    let model_info = async_openai::types::models::Model {
        id: model.model_id.clone(),
        created: 0,
        object: "model".to_string(),
        owned_by: String::new(),
    };

    // 5. Detect features
    let features = detect_model_features(&client, &model_info).await;

    // 6. Update only the feature flags in the database
    let update = UpdateLLMModel {
        model_id: None,
        is_active: None,
        has_image_generation: Some(features.has_image_generation),
        has_speech: Some(features.has_speech),
        has_chat_completion: Some(features.has_chat_completion),
        has_embedding: Some(features.has_embedding),
        has_messages: None,
        has_responses_api: Some(features.has_responses_api),
        input_token_price: None,
        output_token_price: None,
        batch_input_token_price: None,
        batch_output_token_price: None,
        description: None,
        updated_at: Some(Utc::now().naive_utc()),
    };
    let updated_model = db::llm::update_model(pool, model_pk, &update).await?;
    Ok(updated_model)
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
