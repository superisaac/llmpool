use clap::Subcommand;
use serde::Serialize;

use super::{ConsumerResponse, PaginatedResponse, print_pagination, resolve_consumer_id, truncate};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum ConsumerAction {
    /// List all consumers
    List,
    /// Show a consumer's details
    Show {
        /// Consumer name or consumer ID
        #[arg(long)]
        consumer: String,
    },
    /// Add a new consumer
    Add {
        /// Name for the new consumer
        name: String,
    },
    /// Update an existing consumer
    Update {
        /// Name or consumer ID of the consumer to update
        #[arg(long)]
        consumer: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// Whether the consumer is active (true/false)
        #[arg(long)]
        is_active: Option<bool>,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateConsumerRequest {
    name: String,
}

#[derive(Serialize)]
struct UpdateConsumerRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_consumers(consumers: &[ConsumerResponse]) {
    if consumers.is_empty() {
        println!("No consumers found.");
        return;
    }

    println!(
        "{:<5} {:<20} {:<8} {:<22} {:<22}",
        "ID", "Name", "Active", "Created At", "Updated At"
    );
    println!("{}", "-".repeat(80));
    for u in consumers {
        println!(
            "{:<5} {:<20} {:<8} {:<22} {:<22}",
            u.id,
            truncate(&u.name, 18),
            if u.is_active { "yes" } else { "no" },
            u.created_at,
            u.updated_at,
        );
    }
}

fn print_consumer_detail(u: &ConsumerResponse) {
    println!("Consumer created successfully!");
    println!();
    println!("  ID:         {}", u.id);
    println!("  Name:       {}", u.name);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

fn print_consumer_info(u: &ConsumerResponse) {
    println!("  ID:         {}", u.id);
    println!("  Name:       {}", u.name);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_consumer(
    action: ConsumerAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        ConsumerAction::List => {
            if json_output {
                let raw = client.get_raw("/consumers").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<ConsumerResponse> = client.get("/consumers").await?;
                print_consumers(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        ConsumerAction::Show { consumer } => {
            if json_output {
                let path = if let Ok(id) = consumer.parse::<i32>() {
                    format!("/consumers/{}", id)
                } else {
                    format!("/consumers_by_name/{}", consumer)
                };
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: ConsumerResponse = if let Ok(id) = consumer.parse::<i32>() {
                    client.get(&format!("/consumers/{}", id)).await?
                } else {
                    client
                        .get(&format!("/consumers_by_name/{}", consumer))
                        .await?
                };
                print_consumer_info(&resp);
            }
        }
        ConsumerAction::Add { name } => {
            let body = CreateConsumerRequest { name };
            if json_output {
                let raw = client.post_raw("/consumers", &body).await?;
                println!("{}", raw);
            } else {
                let resp: ConsumerResponse = client.post("/consumers", &body).await?;
                print_consumer_detail(&resp);
            }
        }
        ConsumerAction::Update {
            consumer,
            name,
            is_active,
        } => {
            let consumer_id = resolve_consumer_id(&consumer, client).await?;
            let body = UpdateConsumerRequestBody { name, is_active };
            if json_output {
                let raw = client
                    .put_raw(&format!("/consumers/{}", consumer_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: ConsumerResponse = client
                    .put(&format!("/consumers/{}", consumer_id), &body)
                    .await?;
                println!("Consumer updated successfully!");
                println!();
                print_consumer_info(&resp);
            }
        }
    }
    Ok(())
}
