use async_openai::{
    Client,
    config::OpenAIConfig,
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
use crate::models::{NewOpenAIEndpoint, NewOpenAIModel, UpdateOpenAIEndpoint, UpdateOpenAIModel};

pub struct ModelFeatures {
    pub model: Model,
    pub has_image_generation: bool,
    // has_video: bool,
    pub has_speech: bool,
    pub has_embedding: bool,
    pub has_chat_completion: bool,
}

pub struct APIEndpointFeatures {
    pub has_responses_api: bool,
    pub model_features: Vec<ModelFeatures>,
}

pub async fn detect_features(
    api_key: &str,
    api_base: &str,
) -> Result<APIEndpointFeatures, OpenAIError> {
    // Initialize client from environment variables
    let config = OpenAIConfig::new()
        .with_api_key(api_key.to_string())
        .with_api_base(api_base.to_string());
    let client = Client::with_config(config);

    let response = client.models().list().await?;
    let mut models = response.data;

    // Sort by ID
    models.sort_by(|a, b| a.id.cmp(&b.id));

    let has_responses_api = check_responses_api_support(&client).await;

    let mut model_features: Vec<_> = vec![];
    for model in models {
        // Perform live feature detection
        let features = detect_model_features(&client, &model).await;
        model_features.push(features);
    }
    let api_features = APIEndpointFeatures {
        has_responses_api,
        model_features,
    };
    Ok(api_features)
}

/// Detect model features through actual endpoint calls
async fn detect_model_features(
    client: &Client<async_openai::config::OpenAIConfig>,
    model: &Model,
) -> ModelFeatures {
    let has_image_generation = check_image_generation_support(client, model).await;
    let has_speech = check_speech_support(client, model).await;
    let has_embedding = check_embedding_support(client, model).await;
    let has_chat_completion = check_chat_completion_support(client, model).await;

    ModelFeatures {
        model: model.clone(),
        has_image_generation,
        has_speech,
        has_embedding,
        has_chat_completion,
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

async fn check_responses_api_support(client: &Client<async_openai::config::OpenAIConfig>) -> bool {
    match client.responses().retrieve("~nosuchresponse_id").await {
        Ok(_) => true,
        Err(e) => !is_unsupported_error(&e),
    }
}

/// Detect features for an API endpoint and save the results to the database.
/// This will upsert the endpoint (by api_base) and upsert each model (by endpoint_id + model_id).
pub async fn detect_and_save_features(
    pool: &DbPool,
    name: &str,
    api_key: &str,
    api_base: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Detect features from the remote API
    let api_features = detect_features(api_key, api_base).await?;

    // 2. Upsert the OpenAIEndpoint
    let endpoint = match db::openai::get_endpoint_by_api_base(pool, api_base).await {
        Ok(existing) => {
            // Update existing endpoint
            let update = UpdateOpenAIEndpoint {
                name: Some(name.to_string()),
                api_base: None,
                api_key: Some(api_key.to_string()),
                has_responses_api: Some(api_features.has_responses_api),
                updated_at: Some(Utc::now().naive_utc()),
            };
            db::openai::update_endpoint(pool, existing.id, &update).await?
        }
        Err(sqlx::Error::RowNotFound) => {
            // Insert new endpoint
            let new_endpoint = NewOpenAIEndpoint {
                name: name.to_string(),
                api_base: api_base.to_string(),
                api_key: api_key.to_string(),
                has_responses_api: api_features.has_responses_api,
            };
            db::openai::create_endpoint(pool, &new_endpoint).await?
        }
        Err(e) => return Err(Box::new(e)),
    };

    // 3. Upsert each model
    for mf in &api_features.model_features {
        match db::openai::find_model_by_endpoint_and_model_id(pool, endpoint.id, &mf.model.id).await
        {
            Ok(existing_model) => {
                // Update existing model
                let update = UpdateOpenAIModel {
                    model_id: None,
                    has_image_generation: Some(mf.has_image_generation),
                    has_speech: Some(mf.has_speech),
                    has_chat_completion: Some(mf.has_chat_completion),
                    has_embedding: Some(mf.has_embedding),
                    input_token_price: None,
                    output_token_price: None,
                    updated_at: Some(Utc::now().naive_utc()),
                };
                db::openai::update_model(pool, existing_model.id, &update).await?;
            }
            Err(sqlx::Error::RowNotFound) => {
                // Insert new model
                let default_token_price = BigDecimal::from_str("0.000001").unwrap();
                let new_model = NewOpenAIModel {
                    endpoint_id: endpoint.id,
                    model_id: mf.model.id.clone(),
                    has_image_generation: mf.has_image_generation,
                    has_speech: mf.has_speech,
                    has_chat_completion: mf.has_chat_completion,
                    has_embedding: mf.has_embedding,
                    input_token_price: default_token_price.clone(),
                    output_token_price: default_token_price,
                };
                db::openai::create_model(pool, &new_model).await?;
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
