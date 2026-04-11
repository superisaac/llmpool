use async_openai::{Client, config::OpenAIConfig, types::images::CreateImageRequest};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::models::CapacityOption;
use crate::openai::session_tracer::SessionTracer;

/// Handle POST /v1/images/generations upstream (image generation)
pub async fn generate_images(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateImageRequest>,
) -> Response {
    let model_name = image_model_to_string(&payload.model);
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        feature: Some(crate::openai::features::FEATURE_IMAGES.to_string()),
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for image model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, upstream_client) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            upstream_client.model_db_id,
            api_key_id,
        );
        match generate_images_with_client(&upstream_client.client, &mut tracer, payload.clone())
            .await
        {
            Ok(response) => return response,
            Err(e) => {
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Image generation failed, retrying with another upstream"
                    );
                } else {
                    eprintln!("Image generation failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Execute an image generation request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn generate_images_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateImageRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateImageRequest(payload.clone()))
        .await;

    let response = client.images().generate(payload).await?;

    if let Some(ref usage) = response.usage {
        info!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            total_tokens = usage.total_tokens,
            "Image generation usage"
        );
    }

    // Log the response
    tracer
        .add(OpenAIEventData::ImagesResponse(response.clone()))
        .await;

    Ok(Json(response).into_response())
}

/// Convert ImageModel enum to string
fn image_model_to_string(model: &Option<async_openai::types::images::ImageModel>) -> String {
    match model {
        Some(m) => match m {
            async_openai::types::images::ImageModel::GptImage1 => "gpt-image-1".to_string(),
            async_openai::types::images::ImageModel::GptImage1dot5 => "gpt-image-1.5".to_string(),
            async_openai::types::images::ImageModel::GptImage1Mini => {
                "gpt-image-1-mini".to_string()
            }
            async_openai::types::images::ImageModel::DallE2 => "dall-e-2".to_string(),
            async_openai::types::images::ImageModel::DallE3 => "dall-e-3".to_string(),
            async_openai::types::images::ImageModel::Other(s) => s.clone(),
        },
        None => "dall-e-2".to_string(), // default model
    }
}
