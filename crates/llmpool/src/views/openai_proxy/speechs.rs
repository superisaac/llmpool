use async_openai::{Client, config::OpenAIConfig, types::audio::CreateSpeechRequest};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::warn;

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::db;
use crate::middlewares::api_auth::ACCOUNT;
use crate::models::CapacityOption;

/// Handle POST /v1/audio/speech upstream (text-to-speech)
pub async fn create_speech(
    State(state): State<Arc<AppState>>,
    axum::Json(payload): axum::Json<CreateSpeechRequest>,
) -> Response {
    let model_name = speech_model_to_string(&payload.model);
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        feature: Some(crate::openai::features::FEATURE_AUDIO_SPEECH.to_string()),
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 1).await;
    if clients.is_empty() {
        eprintln!("No client for speech model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let upstream_client = &clients[0];
    match create_speech_with_client(&upstream_client.client, payload).await {
        Ok(response) => response,
        Err(e) => {
            // On network errors, mark the upstream as offline
            if matches!(e, async_openai::error::OpenAIError::Reqwest(_)) {
                let pool = state.pool.clone();
                let upstream_id = upstream_client.upstream_id;
                tokio::spawn(async move {
                    if let Err(db_err) = db::llm::mark_upstream_offline(&pool, upstream_id).await {
                        warn!(
                            upstream_id = upstream_id,
                            error = %db_err,
                            "Failed to mark upstream as offline"
                        );
                    } else {
                        warn!(
                            upstream_id = upstream_id,
                            "Marked upstream as offline due to network error"
                        );
                    }
                });
            }
            eprintln!("Speech Generation Error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Execute a speech request using the specified client.
/// Returns Ok(Response) on success, Err(OpenAIError) on failure.
async fn create_speech_with_client(
    client: &Client<OpenAIConfig>,
    payload: CreateSpeechRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    let resp = client.audio().speech().create(payload).await?;
    Ok(Response::builder()
        .header("Content-Type", "audio/mpeg")
        .body(axum::body::Body::from(resp.bytes))
        .unwrap())
}

/// Convert SpeechModel enum to string
fn speech_model_to_string(model: &async_openai::types::audio::SpeechModel) -> String {
    match model {
        async_openai::types::audio::SpeechModel::Tts1 => "tts-1".to_string(),
        async_openai::types::audio::SpeechModel::Tts1Hd => "tts-1-hd".to_string(),
        async_openai::types::audio::SpeechModel::Gpt4oMiniTts => "gpt-4o-mini-tts".to_string(),
        async_openai::types::audio::SpeechModel::Other(s) => s.clone(),
    }
}
