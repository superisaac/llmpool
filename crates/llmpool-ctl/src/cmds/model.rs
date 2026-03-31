use clap::Subcommand;
use serde::Serialize;

use super::{ModelResponse, PaginatedResponse, bool_mark, print_models, print_pagination};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum ModelAction {
    /// List all models
    List,
    /// Update a model
    Update {
        /// ID of the model to update
        #[arg(long)]
        model_id: i32,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Price per input token
        #[arg(long)]
        input_token_price: Option<String>,
        /// Price per output token
        #[arg(long)]
        output_token_price: Option<String>,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct UpdateModelRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_token_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_token_price: Option<f64>,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_model_detail(m: &ModelResponse) {
    println!("Model updated successfully!");
    println!();
    println!("  ID:                {}", m.id);
    println!("  Upstream ID:       {}", m.upstream_id);
    println!("  Model ID:          {}", m.model_id);
    println!("  Chat Completion:   {}", bool_mark(m.has_chat_completion));
    println!("  Embedding:         {}", bool_mark(m.has_embedding));
    println!("  Image Generation:  {}", bool_mark(m.has_image_generation));
    println!("  Speech:            {}", bool_mark(m.has_speech));
    println!("  Input Token Price: {}", m.input_token_price);
    println!("  Output Token Price:{}", m.output_token_price);
    println!("  Description:       {}", m.description);
    println!("  Created At:        {}", m.created_at);
    println!("  Updated At:        {}", m.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_model(
    action: ModelAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        ModelAction::List => {
            if json_output {
                let raw = client.get_raw("/models").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<ModelResponse> = client.get("/models").await?;
                print_models(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        ModelAction::Update {
            model_id,
            description,
            input_token_price,
            output_token_price,
        } => {
            let input_token_price = input_token_price
                .map(|s| s.parse::<f64>())
                .transpose()
                .map_err(|e| format!("Invalid input_token_price: {}", e))?;
            let output_token_price = output_token_price
                .map(|s| s.parse::<f64>())
                .transpose()
                .map_err(|e| format!("Invalid output_token_price: {}", e))?;
            let body = UpdateModelRequestBody {
                description,
                input_token_price,
                output_token_price,
            };
            if json_output {
                let raw = client
                    .put_raw(&format!("/models/{}", model_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: ModelResponse =
                    client.put(&format!("/models/{}", model_id), &body).await?;
                print_model_detail(&resp);
            }
        }
    }
    Ok(())
}
