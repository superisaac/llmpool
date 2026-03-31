use async_openai::{Client, config::OpenAIConfig, types::audio::CreateSpeechRequest};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use super::helpers::{ACCOUNT, AppState, check_fund_balance, select_model_clients};
use crate::models::CapacityOption;

/// Handle POST /v1/audio/speech upstream (text-to-speech)
pub async fn create_speech(
    State(state): State<Arc<AppState>>,
    axum::Json(payload): axum::Json<CreateSpeechRequest>,
) -> Response {
    let model_name = speech_model_to_string(&payload.model);
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        has_speech: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, &model_name, &capacity, 1).await;
    if let Some((client, _model_db_id)) = clients.first() {
        return create_speech_with_client(client, payload).await;
    } else {
        eprintln!("No client for speech model {model_name}");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

/// Execute a speech request using the specified client
async fn create_speech_with_client(
    client: &Client<OpenAIConfig>,
    payload: CreateSpeechRequest,
) -> Response {
    let res = client.audio().speech().create(payload).await;

    match res {
        Ok(resp) => Response::builder()
            .header("Content-Type", "audio/mpeg")
            .body(axum::body::Body::from(resp.bytes))
            .unwrap(),
        Err(e) => {
            eprintln!("Speech Generation Error: {:?}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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
