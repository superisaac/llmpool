use clap::Subcommand;
use serde::Serialize;

use super::{AccountResponse, PaginatedResponse, print_pagination, resolve_account_id, truncate};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum AccountAction {
    /// List all accounts
    List,
    /// Show an account's details
    Show {
        /// Account name or account ID
        #[arg(long)]
        account: String,
    },
    /// Add a new account
    Add {
        /// Name for the new account
        name: String,
    },
    /// Update an existing account
    Update {
        /// Name or account ID of the account to update
        #[arg(long)]
        account: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// Whether the account is active (true/false)
        #[arg(long)]
        is_active: Option<bool>,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateAccountRequest {
    name: String,
}

#[derive(Serialize)]
struct UpdateAccountRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_accounts(accounts: &[AccountResponse]) {
    if accounts.is_empty() {
        println!("No accounts found.");
        return;
    }

    println!(
        "{:<5} {:<20} {:<8} {:<22} {:<22}",
        "ID", "Name", "Active", "Created At", "Updated At"
    );
    println!("{}", "-".repeat(80));
    for u in accounts {
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

fn print_account_detail(u: &AccountResponse) {
    println!("Account created successfully!");
    println!();
    println!("  ID:         {}", u.id);
    println!("  Name:       {}", u.name);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

fn print_account_info(u: &AccountResponse) {
    println!("  ID:         {}", u.id);
    println!("  Name:       {}", u.name);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_account(
    action: AccountAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        AccountAction::List => {
            if json_output {
                let raw = client.get_raw("/accounts").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<AccountResponse> = client.get("/accounts").await?;
                print_accounts(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        AccountAction::Show { account } => {
            if json_output {
                let path = if let Ok(id) = account.parse::<i64>() {
                    format!("/accounts/{}", id)
                } else {
                    format!("/accounts_by_name/{}", account)
                };
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: AccountResponse = if let Ok(id) = account.parse::<i64>() {
                    client.get(&format!("/accounts/{}", id)).await?
                } else {
                    client
                        .get(&format!("/accounts_by_name/{}", account))
                        .await?
                };
                print_account_info(&resp);
            }
        }
        AccountAction::Add { name } => {
            let body = CreateAccountRequest { name };
            if json_output {
                let raw = client.post_raw("/accounts", &body).await?;
                println!("{}", raw);
            } else {
                let resp: AccountResponse = client.post("/accounts", &body).await?;
                print_account_detail(&resp);
            }
        }
        AccountAction::Update {
            account,
            name,
            is_active,
        } => {
            let account_id = resolve_account_id(&account, client).await?;
            let body = UpdateAccountRequestBody { name, is_active };
            if json_output {
                let raw = client
                    .put_raw(&format!("/accounts/{}", account_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: AccountResponse = client
                    .put(&format!("/accounts/{}", account_id), &body)
                    .await?;
                println!("Account updated successfully!");
                println!();
                print_account_info(&resp);
            }
        }
    }
    Ok(())
}
