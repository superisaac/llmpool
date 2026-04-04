use clap::Subcommand;
use serde::Serialize;

use super::{ModelResponse, ModelTestResult, PaginatedResponse, bool_mark, print_models, print_pagination};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum ModelAction {
    /// List all models
    List,
    /// Show details of a specific model
    Show {
        /// Model path (upstream_name/model_name) or model database ID
        #[arg(long)]
        model: String,
    },
    /// Update a model
    Update {
        /// ID of the model to update
        #[arg(long)]
        model_id: i32,
        /// Enable or disable the model
        #[arg(long)]
        is_active: Option<bool>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Price per input token
        #[arg(long)]
        input_token_price: Option<String>,
        /// Price per output token
        #[arg(long)]
        output_token_price: Option<String>,
        /// Price per input token for batch requests
        #[arg(long)]
        batch_input_token_price: Option<String>,
        /// Price per output token for batch requests
        #[arg(long)]
        batch_output_token_price: Option<String>,
    },
    /// Detect features of a model and update its feature flags
    Detect {
        /// Model path (upstream_name/model_name) or model database ID
        #[arg(long)]
        model: String,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct UpdateModelRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_token_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_token_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_input_token_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_output_token_price: Option<String>,
}

#[derive(Serialize)]
struct TestModelsRequestBody {
    model_ids: Vec<i32>,
}

// ============================================================
// Display Helpers
// ============================================================

pub fn print_model_full(m: &ModelResponse, title: &str) {
    println!("{}", title);
    println!();
    println!("  ID:                       {}", m.id);
    println!("  Upstream ID:              {}", m.upstream_id);
    println!("  Model ID:                 {}", m.model_id);
    println!("  Active:                   {}", bool_mark(m.is_active));
    println!("  Chat Completion:          {}", bool_mark(m.has_chat_completion));
    println!("  Embedding:                {}", bool_mark(m.has_embedding));
    println!("  Image Generation:         {}", bool_mark(m.has_image_generation));
    println!("  Speech:                   {}", bool_mark(m.has_speech));
    println!("  Input Token Price:        {}", m.input_token_price);
    println!("  Output Token Price:       {}", m.output_token_price);
    println!("  Batch Input Token Price:  {}", m.batch_input_token_price);
    println!("  Batch Output Token Price: {}", m.batch_output_token_price);
    println!("  Description:              {}", m.description);
    println!("  Created At:               {}", m.created_at);
    println!("  Updated At:               {}", m.updated_at);
}

fn print_model_detect_result(result: &ModelTestResult) {
    match &result.model {
        Some(m) => {
            print_model_full(m, "Model detection completed successfully!");
        }
        None => {
            println!(
                "Model detection failed for model ID {}: {}",
                result.model_id,
                result.error.as_deref().unwrap_or("unknown error")
            );
        }
    }
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
        ModelAction::Show { model } => {
            let model_id = resolve_model_id(&model, client).await?;
            if json_output {
                let raw = client.get_raw(&format!("/models/{}", model_id)).await?;
                println!("{}", raw);
            } else {
                let resp: ModelResponse = client.get(&format!("/models/{}", model_id)).await?;
                print_model_full(&resp, "Model details:");
            }
        }
        ModelAction::Update {
            model_id,
            is_active,
            description,
            input_token_price,
            output_token_price,
            batch_input_token_price,
            batch_output_token_price,
        } => {
            // Validate that prices are valid non-negative decimals if provided
            for (name, price) in [
                ("input_token_price", input_token_price.as_deref()),
                ("output_token_price", output_token_price.as_deref()),
                ("batch_input_token_price", batch_input_token_price.as_deref()),
                ("batch_output_token_price", batch_output_token_price.as_deref()),
            ] {
                if let Some(p) = price {
                    p.parse::<f64>()
                        .map_err(|e| format!("Invalid {}: {}", name, e))?;
                }
            }
            let body = UpdateModelRequestBody {
                is_active,
                description,
                input_token_price,
                output_token_price,
                batch_input_token_price,
                batch_output_token_price,
            };
            if json_output {
                let raw = client
                    .put_raw(&format!("/models/{}", model_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: ModelResponse =
                    client.put(&format!("/models/{}", model_id), &body).await?;
                print_model_full(&resp, "Model updated successfully!");
            }
        }
        ModelAction::Detect { model } => {
            // Resolve model path or ID to a numeric model ID
            let model_id = resolve_model_id(&model, client).await?;

            let body = TestModelsRequestBody {
                model_ids: vec![model_id],
            };

            if json_output {
                let raw = client.post_raw("/models-tests", &body).await?;
                println!("{}", raw);
            } else {
                println!("Detecting features for model {}...", model);
                let results: Vec<ModelTestResult> = client.post("/models-tests", &body).await?;
                println!();
                if let Some(result) = results.first() {
                    print_model_detect_result(result);
                } else {
                    println!("No results returned.");
                }
            }
        }
    }
    Ok(())
}

// ============================================================
// ID Resolution Helper
// ============================================================

/// Resolve a model path (upstream_name/model_name) or numeric ID string to a model database ID.
///
/// If `model` parses as an integer, it is used directly as the model ID.
/// Otherwise, it is treated as a path of the form `upstream_name/model_name` and
/// the API endpoint `GET /api/v1/models/path/{upstream_name}/{model_name}` is called
/// to look up the model ID.
async fn resolve_model_id(model: &str, client: &ApiClient) -> Result<i32, String> {
    // If it's a plain integer, use it directly as the model ID
    if let Ok(id) = model.parse::<i32>() {
        return Ok(id);
    }

    // Otherwise treat it as a path: upstream_name/model_name
    // Split on the first '/' to get upstream_name and model_name
    let slash_pos = model
        .find('/')
        .ok_or_else(|| format!("'{}' is not a valid model ID or path (expected integer or upstream_name/model_name)", model))?;

    let upstream_name = &model[..slash_pos];
    let model_name = &model[slash_pos + 1..];

    if upstream_name.is_empty() || model_name.is_empty() {
        return Err(format!(
            "'{}' is not a valid model path (expected upstream_name/model_name)",
            model
        ));
    }

    let resp: ModelResponse = client
        .get(&format!("/models/path/{}/{}", upstream_name, model_name))
        .await?;

    Ok(resp.id)
}
