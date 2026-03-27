use clap::Subcommand;
use serde::Serialize;

use super::{BalanceChangeResponse, FundResponse, resolve_consumer_id};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum FundAction {
    /// Show consumer fund balance
    Show {
        /// Consumer name or consumer ID
        #[arg(long)]
        consumer: String,
    },
    /// Deposit cash to a consumer's fund
    Deposit {
        /// Consumer name or consumer ID
        #[arg(long)]
        consumer: String,
        /// Amount to deposit
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Withdraw cash from a consumer's fund
    Withdraw {
        /// Consumer name or consumer ID
        #[arg(long)]
        consumer: String,
        /// Amount to withdraw
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Add credit to a consumer's fund
    Credit {
        /// Consumer name or consumer ID
        #[arg(long)]
        consumer: String,
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
    consumer_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateWithdrawRequest {
    consumer_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateCreditRequest {
    consumer_id: i32,
    unique_request_id: String,
    amount: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_fund_detail(f: &FundResponse) {
    println!("Fund for consumer ID {}:", f.consumer_id);
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
    println!("  Consumer ID:       {}", bc.consumer_id);
    println!("  Request ID:        {}", bc.unique_request_id);
    println!("  Content:           {}", bc.content);
    println!(
        "  Applied:           {}",
        if bc.is_applied { "yes" } else { "no" }
    );
    println!("  Created At:        {}", bc.created_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_fund(
    action: FundAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        FundAction::Show { consumer } => {
            let consumer_id = resolve_consumer_id(&consumer, client).await?;
            if json_output {
                let raw = client.get_raw(&format!("/consumers/{}/fund", consumer_id)).await?;
                println!("{}", raw);
            } else {
                let resp: FundResponse = client.get(&format!("/consumers/{}/fund", consumer_id)).await?;
                print_fund_detail(&resp);
            }
        }
        FundAction::Deposit {
            consumer,
            amount,
            request_id,
        } => {
            let consumer_id = resolve_consumer_id(&consumer, client).await?;
            let body = CreateDepositRequest {
                consumer_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/deposits", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client.post("/deposits", &body).await?;
                print_balance_change(&resp, "Deposit");
            }
        }
        FundAction::Withdraw {
            consumer,
            amount,
            request_id,
        } => {
            let consumer_id = resolve_consumer_id(&consumer, client).await?;
            let body = CreateWithdrawRequest {
                consumer_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/withdrawals", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client.post("/withdrawals", &body).await?;
                print_balance_change(&resp, "Withdrawal");
            }
        }
        FundAction::Credit {
            consumer,
            amount,
            request_id,
        } => {
            let consumer_id = resolve_consumer_id(&consumer, client).await?;
            let body = CreateCreditRequest {
                consumer_id,
                unique_request_id: request_id,
                amount,
            };
            if json_output {
                let raw = client.post_raw("/credits", &body).await?;
                println!("{}", raw);
            } else {
                let resp: BalanceChangeResponse = client.post("/credits", &body).await?;
                print_balance_change(&resp, "Credit");
            }
        }
    }
    Ok(())
}
