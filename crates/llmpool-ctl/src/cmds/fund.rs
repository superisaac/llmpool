use clap::Subcommand;
use serde::Serialize;

use super::{BalanceChangeResponse, FundResponse, resolve_account_id};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum FundAction {
    /// Show account fund balance
    Show {
        /// Account name or account ID
        #[arg(long)]
        account: String,
    },
    /// Deposit cash to an account's fund
    Deposit {
        /// Account name or account ID
        #[arg(long)]
        account: String,
        /// Amount to deposit
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Withdraw cash from an account's fund
    Withdraw {
        /// Account name or account ID
        #[arg(long)]
        account: String,
        /// Amount to withdraw
        #[arg(long)]
        amount: String,
        /// Unique request ID for idempotency
        #[arg(long)]
        request_id: String,
    },
    /// Add a credit to an account's fund
    Credit {
        /// Account name or account ID
        #[arg(long)]
        account: String,
        /// Amount to credit
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
    account_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateWithdrawRequest {
    account_id: i32,
    unique_request_id: String,
    amount: String,
}

#[derive(Serialize)]
struct CreateCreditRequest {
    account_id: i32,
    unique_request_id: String,
    amount: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_fund_detail(f: &FundResponse) {
    println!("Fund for account ID {}:", f.account_id);
    println!();
    println!("  Balance:    {}", f.balance);
    if !f.created_at.is_empty() {
        println!("  Created At: {}", f.created_at);
        println!("  Updated At: {}", f.updated_at);
    }
}

fn print_balance_change(bc: &BalanceChangeResponse, action: &str) {
    println!("{} created successfully!", action);
    println!();
    println!("  ID:                {}", bc.id);
    println!("  Account ID:        {}", bc.account_id);
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
        FundAction::Show { account } => {
            let account_id = resolve_account_id(&account, client).await?;
            if json_output {
                let raw = client
                    .get_raw(&format!("/accounts/{}/fund", account_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: FundResponse = client
                    .get(&format!("/accounts/{}/fund", account_id))
                    .await?;
                print_fund_detail(&resp);
            }
        }
        FundAction::Deposit {
            account,
            amount,
            request_id,
        } => {
            let account_id = resolve_account_id(&account, client).await?;
            let body = CreateDepositRequest {
                account_id: account_id,
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
            account,
            amount,
            request_id,
        } => {
            let account_id = resolve_account_id(&account, client).await?;
            let body = CreateWithdrawRequest {
                account_id: account_id,
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
            account,
            amount,
            request_id,
        } => {
            let account_id = resolve_account_id(&account, client).await?;
            let body = CreateCreditRequest {
                account_id: account_id,
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
