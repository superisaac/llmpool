use clap::{Parser, Subcommand};
use std::process;

mod client;
mod cmds;

use cmds::{
    ApiKeyAction, ConsumerAction, EndpointAction, FundAction, ModelAction, handle_apikey,
    handle_consumer, handle_endpoint, handle_fund, handle_model,
};

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Parser)]
#[command(
    name = "llmpool-ctl",
    about = "CLI tool for managing LLMPool via Admin API"
)]
struct Cli {
    /// Output format: "" (default) for human-readable, "json" for JSON
    #[arg(long, default_value = "", global = true)]
    format: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage endpoints
    Endpoint {
        #[command(subcommand)]
        action: EndpointAction,
    },
    /// Manage models
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Manage consumers
    Consumer {
        #[command(subcommand)]
        action: ConsumerAction,
    },
    /// Manage API keys
    Apikey {
        #[command(subcommand)]
        action: ApiKeyAction,
    },
    /// Manage consumer funds (balance, deposit, withdraw, credit)
    Fund {
        #[command(subcommand)]
        action: FundAction,
    },
}

// ============================================================
// Main
// ============================================================

#[tokio::main]
async fn main() {
    // Load .env file if present (ignore errors)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let admin_url = match std::env::var("LLMPOOL_ADMIN_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: LLMPOOL_ADMIN_URL environment variable is not set.");
            eprintln!("Set it directly or add it to a .env file in the current directory.");
            process::exit(1);
        }
    };

    let admin_token = match std::env::var("LLMPOOL_ADMIN_TOKEN") {
        Ok(token) => token,
        Err(_) => {
            eprintln!("Error: LLMPOOL_ADMIN_TOKEN environment variable is not set.");
            eprintln!("Set it directly or add it to a .env file in the current directory.");
            process::exit(1);
        }
    };

    let api_client = client::ApiClient::new(admin_url, admin_token);
    let json_output = cli.format == "json";

    let result = match cli.command {
        Commands::Endpoint { action } => handle_endpoint(action, &api_client, json_output).await,
        Commands::Model { action } => handle_model(action, &api_client, json_output).await,
        Commands::Consumer { action } => handle_consumer(action, &api_client, json_output).await,
        Commands::Apikey { action } => handle_apikey(action, &api_client, json_output).await,
        Commands::Fund { action } => handle_fund(action, &api_client, json_output).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
