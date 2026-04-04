use async_openai::types::models::{ListModelResponse, Model};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashSet;
use std::sync::Arc;

use super::helpers::AppState;
use crate::db;

/// Handle /v1/models upstream, return available model list from database
pub async fn list_merged_models(State(state): State<Arc<AppState>>) -> Response {
    let res = db::llm::list_models(&state.pool).await;

    match res {
        Ok(models) => {
            // Deduplicate by model_id, keeping the first occurrence
            let mut seen = HashSet::new();
            let unique_models: Vec<Model> = models
                .into_iter()
                .filter(|m| seen.insert(m.model_id.clone()))
                .map(|m| Model {
                    id: m.model_id,
                    object: "model".to_string(),
                    created: m.created_at.and_utc().timestamp() as u32,
                    owned_by: "system".to_string(),
                })
                .collect();

            let response = ListModelResponse {
                object: "list".to_string(),
                data: unique_models,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Models Error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
