use async_openai::{Client, config::OpenAIConfig, types::embeddings::CreateEmbeddingRequest};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{info, warn};

use super::helpers::{AppState, check_wallet_balance, select_model_clients};
use crate::db;
use crate::defer::OpenAIEventData;
use crate::middlewares::api_auth::{ACCOUNT, API_CREDENTIAL};
use crate::openai::session_tracer::SessionTracer;

/// Handle POST /v1/embeddings upstream
pub async fn create_embeddings(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateEmbeddingRequest>,
) -> Response {
    let model_name = &payload.model;
    let account_id = ACCOUNT.with(|u| u.id);

    // Check if the account has sufficient wallets
    if let Err(resp) = check_wallet_balance(&state, account_id).await {
        return resp;
    }

    let clients = select_model_clients(
        &state.pool,
        &state.redis_pool,
        model_name,
        "openai",
        crate::openai::features::FEATURE_EMBEDDINGS,
        2,
    )
    .await;
    if clients.is_empty() {
        eprintln!("No client for embedding model {model_name}");
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
        let mut upstream_payload = payload.clone();
        upstream_payload.model = upstream_client.fullname.clone();
        match create_embeddings_with_client(&upstream_client.client, &mut tracer, upstream_payload)
            .await
        {
            Ok(response) => return response,
            Err(e) => {
                // On network errors, mark the upstream as offline
                if matches!(e, async_openai::error::OpenAIError::Reqwest(_)) {
                    let pool = state.pool.clone();
                    let upstream_id = upstream_client.upstream_id;
                    tokio::spawn(async move {
                        if let Err(db_err) =
                            db::llm::mark_upstream_offline(&pool, upstream_id).await
                        {
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
