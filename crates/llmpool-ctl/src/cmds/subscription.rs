use clap::Subcommand;
use serde::{Deserialize, Serialize};

use super::{PaginatedResponse, print_pagination, resolve_account_id, truncate};
use crate::client::ApiClient;

// ============================================================
// Response Types
// ============================================================

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SubscriptionPlanResponse {
    pub id: i32,
    pub status: String,
    pub description: String,
    pub input_token_limit: i64,
    pub output_token_limit: i64,
    pub money_limit: String,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SubscriptionResponse {
    pub id: i32,
    pub account_id: i32,
    pub plan_id: i32,
    pub status: String,
    pub used_input_tokens: i64,
    pub used_output_tokens: i64,
    pub used_money: String,
    pub created_at: String,
    pub updated_at: String,
}

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum SubscriptionPlanAction {
    /// List all subscription plans
    List,
    /// Show a subscription plan's details
    Show {
        /// Subscription plan ID
        #[arg(long)]
        plan_id: i32,
    },
    /// Add a new subscription plan
    Add {
        /// Description of the plan
        #[arg(long)]
        description: String,
        /// Input token limit (0 = unlimited)
        #[arg(long, default_value = "0")]
        input_token_limit: i64,
        /// Output token limit (0 = unlimited)
        #[arg(long, default_value = "0")]
        output_token_limit: i64,
        /// Money limit (0 = unlimited)
        #[arg(long, default_value = "0")]
        money_limit: String,
        /// Start datetime (e.g. 2024-01-01T00:00:00)
        #[arg(long)]
        start_at: Option<String>,
        /// End datetime (e.g. 2024-12-31T23:59:59)
        #[arg(long)]
        end_at: Option<String>,
        /// Sort order (higher = higher priority)
        #[arg(long, default_value = "0")]
        sort_order: i32,
    },
    /// Update a subscription plan
    Update {
        /// Subscription plan ID
        #[arg(long)]
        plan_id: i32,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New input token limit
        #[arg(long)]
        input_token_limit: Option<i64>,
        /// New output token limit
        #[arg(long)]
        output_token_limit: Option<i64>,
        /// New money limit
        #[arg(long)]
        money_limit: Option<String>,
        /// New sort order
        #[arg(long)]
        sort_order: Option<i32>,
        /// New status (created, started, deducted, active, canceled, expired)
        #[arg(long)]
        status: Option<String>,
    },
    /// Cancel a subscription plan
    Cancel {
        /// Subscription plan ID
        #[arg(long)]
        plan_id: i32,
    },
}

#[derive(Subcommand)]
pub enum SubscriptionAction {
    /// List subscriptions (optionally filter by account or status)
    List {
        /// Filter by account name or ID
        #[arg(long)]
        account: Option<String>,
        /// Filter by status (active, filled, canceled)
        #[arg(long)]
        status: Option<String>,
    },
    /// Show a subscription's details
    Show {
        /// Subscription ID
        #[arg(long)]
        subscription_id: i32,
    },
    /// Create a new subscription for an account
    Add {
        /// Account name or ID
        #[arg(long)]
        account: String,
        /// Subscription plan ID
        #[arg(long)]
        plan_id: i32,
    },
    /// Update a subscription's status
    Update {
        /// Subscription ID
        #[arg(long)]
        subscription_id: i32,
        /// New status (active, filled, canceled)
        #[arg(long)]
        status: String,
    },
    /// Cancel a subscription
    Cancel {
        /// Subscription ID
        #[arg(long)]
        subscription_id: i32,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateSubscriptionPlanRequest {
    description: String,
    input_token_limit: i64,
    output_token_limit: i64,
    money_limit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_at: Option<String>,
    sort_order: i32,
}

#[derive(Serialize)]
struct UpdateSubscriptionPlanRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_token_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_token_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    money_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

#[derive(Serialize)]
struct CreateSubscriptionRequest {
    account_id: i32,
    plan_id: i32,
}

#[derive(Serialize)]
struct UpdateSubscriptionRequest {
    status: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_plans(plans: &[SubscriptionPlanResponse]) {
    if plans.is_empty() {
        println!("No subscription plans found.");
        return;
    }
    println!(
        "{:<5} {:<10} {:<25} {:<15} {:<15} {:<12} {:<5}",
        "ID", "Status", "Description", "Input Limit", "Output Limit", "Money Limit", "Order"
    );
    println!("{}", "-".repeat(90));
    for p in plans {
        println!(
            "{:<5} {:<10} {:<25} {:<15} {:<15} {:<12} {:<5}",
            p.id,
            p.status,
            truncate(&p.description, 23),
            p.input_token_limit,
            p.output_token_limit,
            truncate(&p.money_limit, 10),
            p.sort_order,
        );
    }
}

fn print_plan_detail(p: &SubscriptionPlanResponse) {
    println!("  ID:                  {}", p.id);
    println!("  Status:              {}", p.status);
    println!("  Description:         {}", p.description);
    println!("  Input Token Limit:   {}", p.input_token_limit);
    println!("  Output Token Limit:  {}", p.output_token_limit);
    println!("  Money Limit:         {}", p.money_limit);
    println!(
        "  Start At:            {}",
        p.start_at.as_deref().unwrap_or("(none)")
    );
    println!(
        "  End At:              {}",
        p.end_at.as_deref().unwrap_or("(none)")
    );
    println!("  Sort Order:          {}", p.sort_order);
    println!("  Created At:          {}", p.created_at);
    println!("  Updated At:          {}", p.updated_at);
}

fn print_subscriptions(subs: &[SubscriptionResponse]) {
    if subs.is_empty() {
        println!("No subscriptions found.");
        return;
    }
    println!(
        "{:<5} {:<10} {:<8} {:<10} {:<15} {:<15} {:<12} {:<22}",
        "ID", "Status", "Acct ID", "Plan ID", "Used Input", "Used Output", "Used Money", "Created At"
    );
    println!("{}", "-".repeat(100));
    for s in subs {
        println!(
            "{:<5} {:<10} {:<8} {:<10} {:<15} {:<15} {:<12} {:<22}",
            s.id,
            s.status,
            s.account_id,
            s.plan_id,
            s.used_input_tokens,
            s.used_output_tokens,
            truncate(&s.used_money, 10),
            s.created_at,
        );
    }
}

fn print_subscription_detail(s: &SubscriptionResponse) {
    println!("  ID:                  {}", s.id);
    println!("  Account ID:          {}", s.account_id);
    println!("  Plan ID:             {}", s.plan_id);
    println!("  Status:              {}", s.status);
    println!("  Used Input Tokens:   {}", s.used_input_tokens);
    println!("  Used Output Tokens:  {}", s.used_output_tokens);
    println!("  Used Money:          {}", s.used_money);
    println!("  Created At:          {}", s.created_at);
    println!("  Updated At:          {}", s.updated_at);
}

// ============================================================
// Command Handlers
// ============================================================

pub async fn handle_subscription_plan(
    action: SubscriptionPlanAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        SubscriptionPlanAction::List => {
            if json_output {
                let raw = client.get_raw("/subscription-plans").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<SubscriptionPlanResponse> =
                    client.get("/subscription-plans").await?;
                print_plans(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        SubscriptionPlanAction::Show { plan_id } => {
            if json_output {
                let raw = client
                    .get_raw(&format!("/subscription-plans/{}", plan_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionPlanResponse = client
                    .get(&format!("/subscription-plans/{}", plan_id))
                    .await?;
                print_plan_detail(&resp);
            }
        }
        SubscriptionPlanAction::Add {
            description,
            input_token_limit,
            output_token_limit,
            money_limit,
            start_at,
            end_at,
            sort_order,
        } => {
            let body = CreateSubscriptionPlanRequest {
                description,
                input_token_limit,
                output_token_limit,
                money_limit,
                start_at,
                end_at,
                sort_order,
            };
            if json_output {
                let raw = client.post_raw("/subscription-plans", &body).await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionPlanResponse =
                    client.post("/subscription-plans", &body).await?;
                println!("Subscription plan created successfully!");
                println!();
                print_plan_detail(&resp);
            }
        }
        SubscriptionPlanAction::Update {
            plan_id,
            description,
            input_token_limit,
            output_token_limit,
            money_limit,
            sort_order,
            status,
        } => {
            let body = UpdateSubscriptionPlanRequest {
                description,
                input_token_limit,
                output_token_limit,
                money_limit,
                sort_order,
                status,
            };
            if json_output {
                let raw = client
                    .put_raw(&format!("/subscription-plans/{}", plan_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionPlanResponse = client
                    .put(&format!("/subscription-plans/{}", plan_id), &body)
                    .await?;
                println!("Subscription plan updated successfully!");
                println!();
                print_plan_detail(&resp);
            }
        }
        SubscriptionPlanAction::Cancel { plan_id } => {
            if json_output {
                let raw = client
                    .delete_raw(&format!("/subscription-plans/{}", plan_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionPlanResponse = client
                    .delete(&format!("/subscription-plans/{}", plan_id))
                    .await?;
                println!("Subscription plan canceled.");
                println!();
                print_plan_detail(&resp);
            }
        }
    }
    Ok(())
}

pub async fn handle_subscription(
    action: SubscriptionAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        SubscriptionAction::List { account, status } => {
            let mut path = "/subscriptions".to_string();
            let mut query_parts: Vec<String> = Vec::new();

            if let Some(acct) = account {
                let account_id = resolve_account_id(&acct, client).await?;
                query_parts.push(format!("account_id={}", account_id));
            }
            if let Some(s) = status {
                query_parts.push(format!("status={}", s));
            }
            if !query_parts.is_empty() {
                path = format!("{}?{}", path, query_parts.join("&"));
            }

            if json_output {
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<SubscriptionResponse> = client.get(&path).await?;
                print_subscriptions(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        SubscriptionAction::Show { subscription_id } => {
            if json_output {
                let raw = client
                    .get_raw(&format!("/subscriptions/{}", subscription_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionResponse = client
                    .get(&format!("/subscriptions/{}", subscription_id))
                    .await?;
                print_subscription_detail(&resp);
            }
        }
        SubscriptionAction::Add { account, plan_id } => {
            let account_id = resolve_account_id(&account, client).await?;
            let body = CreateSubscriptionRequest { account_id, plan_id };
            if json_output {
                let raw = client.post_raw("/subscriptions", &body).await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionResponse = client.post("/subscriptions", &body).await?;
                println!("Subscription created successfully!");
                println!();
                print_subscription_detail(&resp);
            }
        }
        SubscriptionAction::Update {
            subscription_id,
            status,
        } => {
            let body = UpdateSubscriptionRequest { status };
            if json_output {
                let raw = client
                    .put_raw(&format!("/subscriptions/{}", subscription_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionResponse = client
                    .put(&format!("/subscriptions/{}", subscription_id), &body)
                    .await?;
                println!("Subscription updated successfully!");
                println!();
                print_subscription_detail(&resp);
            }
        }
        SubscriptionAction::Cancel { subscription_id } => {
            if json_output {
                let raw = client
                    .delete_raw(&format!("/subscriptions/{}", subscription_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: SubscriptionResponse = client
                    .delete(&format!("/subscriptions/{}", subscription_id))
                    .await?;
                println!("Subscription canceled.");
                println!();
                print_subscription_detail(&resp);
            }
        }
    }
    Ok(())
}
