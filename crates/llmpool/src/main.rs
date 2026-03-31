use llmpool::config;
use llmpool::db;
use llmpool::defer;
use llmpool::models;
use llmpool::openai;
use llmpool::server;

use clap::{Parser, Subcommand};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "llmpool", about = "LLM Pool Server")]
struct Cli {
    /// Path to the TOML configuration file
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Serve {
        /// Bind address in HOST:PORT format
        #[arg(long, default_value = "127.0.0.1:19324")]
        bind: String,
    },
    /// OpenAI upstream management
    Openai {
        #[command(subcommand)]
        command: OpenaiCommands,
    },
    /// Admin operations
    Admin {
        #[command(subcommand)]
        command: AdminCommands,
    },
    /// Start the deferred task queue worker
    Worker {
        /// Number of concurrent tasks to process
        #[arg(long, default_value = "4")]
        concurrency: usize,
    },
    /// Run database migrations
    Migrate,
    /// Open an interactive database shell (launches psql with the configured database URL)
    Dbshell,
}

#[derive(Subcommand)]
enum AdminCommands {
    /// Generate a JWT token for admin API authentication
    CreateJwtToken {
        /// Token expiration time in seconds (e.g., 3600 for 1 hour). If not specified, the token will not expire.
        #[arg(long)]
        expire: Option<u64>,
        /// Subject claim for the JWT token
        #[arg(long, default_value = "admin")]
        subject: String,
    },
    /// Create a new API key for a user
    CreateApiKey {
        /// The name to create the API key for
        name: String,
    },
    /// Create a user interactively
    CreateUser,
}

/// JWT claims used for admin token generation
#[derive(Debug, Serialize)]
struct AdminClaims {
    sub: String,
    realm: String,
    iat: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    exp: Option<u64>,
}

#[derive(Subcommand)]
enum OpenaiCommands {
    /// Add an OpenAI-compatible upstream by detecting and saving its features
    Add {
        /// A friendly name for this upstream
        #[arg(long)]
        name: String,
        /// The API key for authentication
        #[arg(long)]
        api_key: String,
        /// The base URL of the API upstream
        #[arg(long)]
        api_base: String,
    },
    /// Detect features of an OpenAI-compatible upstream and print results
    Detect {
        /// The API key for authentication
        #[arg(long)]
        api_key: String,
        /// The base URL of the API upstream
        #[arg(long)]
        api_base: String,
    },
}

/// Prompt the user for input with the given message and return the trimmed response
fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().expect("Failed to flush stdout");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Load configuration from TOML file
    let config_path = config::resolve_config_path(cli.config.as_deref());
    config::load_config(&config_path);

    match cli.command {
        Commands::Serve { bind } => {
            server::serve(&bind).await;
        }
        Commands::Migrate => {
            let pool = db::create_pool_from_config().await;
            db::run_migrations(&pool).await;
            println!("Database migrations completed successfully.");
        }
        Commands::Openai { command } => match command {
            OpenaiCommands::Add {
                name,
                api_key,
                api_base,
            } => {
                let pool = db::create_pool_from_config().await;
                match openai::features::detect_and_save_features(&pool, &name, &api_key, &api_base)
                    .await
                {
                    Ok(()) => {
                        println!(
                            "Successfully detected and saved features for upstream '{}'",
                            name
                        );
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            OpenaiCommands::Detect { api_key, api_base } => {
                match openai::features::detect_features(&api_key, &api_base).await {
                    Ok(features) => {
                        println!("API Upstream Features for: {}", api_base);
                        println!("Responses API supported: {}", features.has_responses_api);
                        println!();
                        println!(
                            "{:<40} | {:<10} | {:<10} | {:<15} | {:<10}",
                            "Model ID", "Chat", "Embedding", "Image Gen", "Speech"
                        );
                        println!(
                            "{:-<40}-+-{:-<10}-+-{:-<10}-+-{:-<15}-+-{:-<10}",
                            "", "", "", "", ""
                        );
                        for mf in &features.model_features {
                            println!(
                                "{:<40} | {:<10} | {:<10} | {:<15} | {:<10}",
                                mf.model.id,
                                if mf.has_chat_completion { "✓" } else { "✗" },
                                if mf.has_embedding { "✓" } else { "✗" },
                                if mf.has_image_generation {
                                    "✓"
                                } else {
                                    "✗"
                                },
                                if mf.has_speech { "✓" } else { "✗" },
                            );
                        }
                        println!();
                        println!("Total models: {}", features.model_features.len());
                    }
                    Err(e) => {
                        eprintln!("Error detecting features: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Admin { command } => match command {
            AdminCommands::CreateJwtToken { expire, subject } => {
                let cfg = config::get_config();
                let jwt_secret = &cfg.admin.jwt_secret;

                if jwt_secret.is_empty() {
                    eprintln!("Error: admin.jwt_secret is not configured in the config file");
                    std::process::exit(1);
                }

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                let exp = expire.map(|seconds| now + seconds);

                let claims = AdminClaims {
                    sub: subject,
                    realm: "api".to_string(),
                    iat: now,
                    exp,
                };

                let token = encode(
                    &Header::default(),
                    &claims,
                    &EncodingKey::from_secret(jwt_secret.as_bytes()),
                )
                .unwrap_or_else(|e| {
                    eprintln!("Error generating JWT token: {}", e);
                    std::process::exit(1);
                });

                println!("{}", token);
                if let Some(seconds) = expire {
                    eprintln!("Token expires in {} seconds", seconds);
                } else {
                    eprintln!("Token does not expire");
                }
            }
            AdminCommands::CreateApiKey { name } => {
                let pool = db::create_pool_from_config().await;

                // Find the account by name
                let account = match db::account::get_account_by_name(&pool, &name).await {
                    Ok(Some(account)) => account,
                    Ok(None) => {
                        eprintln!("Error: account '{}' not found", name);
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Error looking up account '{}': {}", name, e);
                        std::process::exit(1);
                    }
                };

                // Create the API key
                match db::api::create_api_credential_for_account(&pool, account.id, "").await {
                    Ok(api_key) => {
                        println!(
                            "API key created for account '{}' (id={})",
                            account.name, account.id
                        );
                        println!("Key: {}", api_key.apikey);
                    }
                    Err(e) => {
                        eprintln!("Error creating API key for account '{}': {}", name, e);
                        std::process::exit(1);
                    }
                }
            }
            AdminCommands::CreateUser => {
                let pool = db::create_pool_from_config().await;

                // Prompt for name
                let name = prompt_input("Name: ");
                if name.is_empty() {
                    eprintln!("Error: name cannot be empty");
                    std::process::exit(1);
                }

                // Create the account
                let new_account = models::NewAccount { name: name.clone() };

                match db::account::create_account(&pool, &new_account).await {
                    Ok(account) => {
                        println!(
                            "Successfully created account '{}' (id={})",
                            account.name, account.id
                        );
                    }
                    Err(e) => {
                        eprintln!("Error creating account '{}': {}", name, e);
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Dbshell => {
            // Resolve database URL the same way as create_pool_from_config
            let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                let cfg = config::get_config();
                cfg.database.url.clone()
            });

            // Launch psql with the configured database URL
            let status = std::process::Command::new("psql")
                .arg(&database_url)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            match status {
                Ok(exit_status) => {
                    if !exit_status.success() {
                        std::process::exit(exit_status.code().unwrap_or(1));
                    }
                }
                Err(e) => {
                    eprintln!("Failed to launch psql: {}", e);
                    eprintln!("Make sure psql is installed and available in your PATH.");
                    std::process::exit(1);
                }
            }
        }
        Commands::Worker { concurrency } => {
            defer::worker::run_worker(concurrency).await;
        }
    }
}
