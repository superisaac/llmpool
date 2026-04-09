use clap::{Parser, Subcommand};
use std::process;

mod client;
mod cmds;

use cmds::{
    AccountAction, ApiKeyAction, FundAction, ModelAction, SessionEventAction, SubscriptionAction,
    SubscriptionPlanAction, UpstreamAction, handle_account, handle_apikey, handle_fund,
    handle_model, handle_session_event, handle_subscription, handle_subscription_plan,
    handle_upstream,
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
    /// Manage upstreams
    Upstream {
        #[command(subcommand)]
        action: UpstreamAction,
    },
    /// Manage models
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Manage accounts
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Manage API keys
    Apikey {
        #[command(subcommand)]
        action: ApiKeyAction,
    },
    /// Manage account funds (balance, deposit, withdraw, credit)
    Fund {
        #[command(subcommand)]
        action: FundAction,
    },
    /// Manage session events
    Sessionevents {
        #[command(subcommand)]
        action: SessionEventAction,
    },
    /// Manage subscription plans
    SubscriptionPlan {
        #[command(subcommand)]
        action: SubscriptionPlanAction,
    },
    /// Manage subscriptions
    Subscription {
        #[command(subcommand)]
        action: SubscriptionAction,
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
        Commands::Upstream { action } => handle_upstream(action, &api_client, json_output).await,
        Commands::Model { action } => handle_model(action, &api_client, json_output).await,
        Commands::Account { action } => handle_account(action, &api_client, json_output).await,
        Commands::Apikey { action } => handle_apikey(action, &api_client, json_output).await,
        Commands::Fund { action } => handle_fund(action, &api_client, json_output).await,
        Commands::Sessionevents { action } => {
            handle_session_event(action, &api_client, json_output).await
        }
        Commands::SubscriptionPlan { action } => {
            handle_subscription_plan(action, &api_client, json_output).await
        }
        Commands::Subscription { action } => {
            handle_subscription(action, &api_client, json_output).await
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
