use async_openai::{Client, config::OpenAIConfig, types::embeddings::CreateEmbeddingRequest};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use super::helpers::{ACCOUNT, API_CREDENTIAL, AppState, check_fund_balance, select_model_clients};
use crate::defer::OpenAIEventData;
use crate::models::CapacityOption;
use crate::openai::session_tracer::SessionTracer;

/// Handle POST /v1/embeddings upstream
pub async fn create_embeddings(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateEmbeddingRequest>,
) -> Response {
    let model_name = &payload.model;
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient funds
    if let Err(resp) = check_fund_balance(&state, account_id).await {
        return resp;
    }

    let capacity = CapacityOption {
        has_embedding: Some(true),
        ..Default::default()
    };
    let clients =
        select_model_clients(&state.pool, &state.redis_pool, model_name, &capacity, 2).await;
    if clients.is_empty() {
        eprintln!("No client for embedding model {model_name}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let api_key_id = API_CREDENTIAL.with(|k| k.id);

    for (i, (client, model_db_id)) in clients.iter().enumerate() {
        let mut tracer = SessionTracer::new(
            state.event_storage.clone(),
            account_id,
            *model_db_id,
            api_key_id,
        );
        match create_embeddings_with_client(client, &mut tracer, payload.clone()).await {
            Ok(response) => return response,
            Err(e) => {
                if i < clients.len() - 1 {
                    warn!(
                        model = model_name,
                        error = %e,
                        "Embedding creation failed, retrying with another upstream"
                    );
                } else {
                    eprintln!("Embedding creation failed after retry: {:?}", e);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
        }
    }
    unreachable!()
}

/// Execute an embedding request using the specified client.
/// Returns Ok(Response) on success, Err on failure so the caller can retry.
async fn create_embeddings_with_client(
    client: &Client<OpenAIConfig>,
    tracer: &mut SessionTracer,
    payload: CreateEmbeddingRequest,
) -> Result<Response, async_openai::error::OpenAIError> {
    // Log the incoming request
    tracer
        .add(OpenAIEventData::CreateEmbeddingRequest(payload.clone()))
        .await;

    let response = client.embeddings().create(payload).await?;

    info!(
        prompt_tokens = response.usage.prompt_tokens,
        total_tokens = response.usage.total_tokens,
        "Embedding usage"
    );

    // Log the response
    tracer
        .add(OpenAIEventData::CreateEmbeddingResponse(response.clone()))
        .await;

    Ok(Json(response).into_response())
}
