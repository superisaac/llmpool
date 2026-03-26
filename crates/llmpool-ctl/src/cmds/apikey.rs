use clap::Subcommand;
use serde::Serialize;

use crate::client::ApiClient;
use super::{
    OpenAIAPIKeyResponse, PaginatedResponse,
    print_pagination, truncate,
    resolve_user_id,
};

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum ApiKeyAction {
    /// List API keys for a user
    List {
        /// Username or user ID
        #[arg(long)]
        user: String,
    },
    /// Add a new API key for a user
    Add {
        /// Username or user ID
        #[arg(long)]
        user: String,
        /// Label describing the purpose of this API key
        #[arg(long, default_value = "")]
        label: String,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateApiKeyRequestBody {
    label: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_apikeys(keys: &[OpenAIAPIKeyResponse]) {
    if keys.is_empty() {
        println!("No API keys found.");
        return;
    }

    println!(
        "{:<5} {:<38} {:<20} {:<8} {:<22}",
        "ID", "API Key", "Label", "Active", "Created At"
    );
    println!("{}", "-".repeat(95));
    for ak in keys {
        println!(
            "{:<5} {:<38} {:<20} {:<8} {:<22}",
            ak.id,
            truncate(&ak.apikey, 36),
            truncate(&ak.label, 18),
            if ak.is_active { "yes" } else { "no" },
            ak.created_at,
        );
    }
}

fn print_apikey_detail(ak: &OpenAIAPIKeyResponse) {
    println!("API key created successfully!");
    println!();
    println!("  ID:         {}", ak.id);
    println!("  API Key:    {}", ak.apikey);
    println!("  Label:      {}", ak.label);
    println!("  Active:     {}", if ak.is_active { "yes" } else { "no" });
    if let Some(ref expires) = ak.expires_at {
        println!("  Expires At: {}", expires);
    }
    println!("  Created At: {}", ak.created_at);
    println!("  Updated At: {}", ak.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_apikey(action: ApiKeyAction, client: &ApiClient, json_output: bool) -> Result<(), String> {
    match action {
        ApiKeyAction::List { user } => {
            let user_id = resolve_user_id(&user, client).await?;
            if json_output {
                let raw = client.get_raw(&format!("/users/{}/apikeys", user_id)).await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<OpenAIAPIKeyResponse> = client
                    .get(&format!("/users/{}/apikeys", user_id))
                    .await?;
                print_apikeys(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        ApiKeyAction::Add { user, label } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = CreateApiKeyRequestBody { label };
            if json_output {
                let raw = client.post_raw(&format!("/users/{}/apikeys", user_id), &body).await?;
                println!("{}", raw);
            } else {
                let resp: OpenAIAPIKeyResponse = client
                    .post(&format!("/users/{}/apikeys", user_id), &body)
                    .await?;
                print_apikey_detail(&resp);
            }
        }
    }
    Ok(())
}
