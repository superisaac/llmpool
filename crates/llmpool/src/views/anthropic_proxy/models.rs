use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::helpers::AnthropicAppState;
use crate::db;
use crate::models::llm::CapacityOption;

/// A single model entry in the Anthropic models list response.
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicModel {
    /// The model identifier (e.g. "claude-3-5-sonnet-20241022")
    pub id: String,
    /// Human-readable display name
    pub display_name: String,
    /// ISO 8601 creation timestamp
    pub created_at: String,
    /// Always "model"
    #[serde(rename = "type")]
    pub object_type: String,
}

/// The response body for GET /v1/models (Anthropic format).
#[derive(Debug, Serialize, Deserialize)]
pub struct ListModelsResponse {
    pub data: Vec<AnthropicModel>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

/// Handle GET /v1/models — return available model list from database in Anthropic format.
pub async fn list_models(State(state): State<Arc<AnthropicAppState>>) -> Response {
    let capacity = CapacityOption {
        feature: Some(crate::anthropic::features::FEATURE_MESSAGES.to_string()),
    };
    let res = db::llm::list_models(&state.pool, &capacity).await;

    match res {
        Ok(models) => {
            // Deduplicate by cname, keeping the first occurrence
            let mut seen = HashSet::new();
            let unique_models: Vec<AnthropicModel> = models
                .into_iter()
                .filter(|m| seen.insert(m.cname.clone()))
                .map(|m| AnthropicModel {
                    id: m.cname.clone(),
                    display_name: m.cname,
                    created_at: m.created_at.and_utc().to_rfc3339(),
                    object_type: "model".to_string(),
                })
                .collect();

            let first_id = unique_models.first().map(|m| m.id.clone());
            let last_id = unique_models.last().map(|m| m.id.clone());

            let response = ListModelsResponse {
                data: unique_models,
                has_more: false,
                first_id,
                last_id,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Anthropic Models Error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
