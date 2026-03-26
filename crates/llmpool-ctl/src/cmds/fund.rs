use clap::Subcommand;
use serde::Serialize;

use crate::client::ApiClient;
use super::{
    BalanceChangeResponse, FundResponse,
    resolve_user_id,
};

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum FundAction {
    /// Show user fund balance
    Show {
        /// Username or user ID
        #[arg(long)]
        user: String,
    },
    /// Deposit cash to a user's fund
    Deposit {
        /// Username or user ID
        #[arg(long)]
        user: String,
        /// Amount to deposit
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Withdraw cash from a user's fund
    Withdraw {
        /// Username or user ID
        #[arg(long)]
        user: String,
        /// Amount to withdraw
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Add credit to a user's fund
    Credit {
        /// Username or user ID
        #[arg(long)]
        user: String,
        /// Amount of credit to add
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateDepositRequest {
    user_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateWithdrawRequest {
    user_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateCreditRequest {
    user_id: i32,
    unique_request_id: String,
    amount: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_fund_detail(f: &FundResponse) {
    println!("Fund for user ID {}:", f.user_id);
    println!();
    println!("  Cash:       {}", f.cash);
    println!("  Credit:     {}", f.credit);
    println!("  Debt:       {}", f.debt);
    if !f.created_at.is_empty() {
        println!("  Created At: {}", f.created_at);
        println!("  Updated At: {}", f.updated_at);
    }
}

fn print_balance_change(bc: &BalanceChangeResponse, action: &str) {
    println!("{} created successfully!", action);
    println!();
    println!("  ID:                {}", bc.id);
    println!("  User ID:           {}", bc.user_id);
    println!("  Request ID:        {}", bc.unique_request_id);
    println!("  Content:           {}", bc.content);
    println!("  Applied:           {}", if bc.is_applied { "yes" } else { "no" });
    println!("  Created At:        {}", bc.created_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_fund(action: FundAction, client: &ApiClient, json_output: bool) -> Result<(), String> {
    match action {
        FundAction::Show { user } => {
            let user_id = resolve_user_id(&user, client).await?;
            if json_output {
                let raw = client.get_raw(&format!("/users/{}/fund", user_id)).await?;
                println!("{}", raw);
            } else {
                let resp: FundResponse = client
                    .get(&format!("/users/{}/fund", user_id))
                    .await?;
                print_fund_detail(&resp);
            }
        }
        FundAction::Deposit {
            user,
            amount,
            request_id,
        } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = CreateDepositRequest {
                user_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/deposits", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client
                    .post("/deposits", &body)
                    .await?;
                print_balance_change(&resp, "Deposit");
            }
        }
        FundAction::Withdraw {
            user,
            amount,
            request_id,
        } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = CreateWithdrawRequest {
                user_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/withdrawals", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client
                    .post("/withdrawals", &body)
                    .await?;
                print_balance_change(&resp, "Withdrawal");
            }
        }
        FundAction::Credit {
            user,
            amount,
            request_id,
        } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = CreateCreditRequest {
                user_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/credits", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client
                    .post("/credits", &body)
                    .await?;
                print_balance_change(&resp, "Credit");
            }
        }
    }
    Ok(())
}
