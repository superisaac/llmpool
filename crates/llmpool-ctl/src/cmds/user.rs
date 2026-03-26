use clap::Subcommand;
use serde::Serialize;

use crate::client::ApiClient;
use super::{
    PaginatedResponse, UserResponse,
    print_pagination, truncate,
    resolve_user_id,
};

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum UserAction {
    /// List all users
    List,
    /// Show a user's details
    Show {
        /// Username or user ID
        #[arg(long)]
        user: String,
    },
    /// Add a new user
    Add {
        /// Username for the new user
        username: String,
    },
    /// Update an existing user
    Update {
        /// Username or user ID of the user to update
        #[arg(long)]
        user: String,
        /// New username
        #[arg(long)]
        username: Option<String>,
        /// Whether the user is active (true/false)
        #[arg(long)]
        is_active: Option<bool>,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateUserRequest {
    username: String,
}

#[derive(Serialize)]
struct UpdateUserRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_active: Option<bool>,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_users(users: &[UserResponse]) {
    if users.is_empty() {
        println!("No users found.");
        return;
    }

    println!(
        "{:<5} {:<20} {:<8} {:<22} {:<22}",
        "ID", "Username", "Active", "Created At", "Updated At"
    );
    println!("{}", "-".repeat(80));
    for u in users {
        println!(
            "{:<5} {:<20} {:<8} {:<22} {:<22}",
            u.id,
            truncate(&u.username, 18),
            if u.is_active { "yes" } else { "no" },
            u.created_at,
            u.updated_at,
        );
    }
}

fn print_user_detail(u: &UserResponse) {
    println!("User created successfully!");
    println!();
    println!("  ID:         {}", u.id);
    println!("  Username:   {}", u.username);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

fn print_user_info(u: &UserResponse) {
    println!("  ID:         {}", u.id);
    println!("  Username:   {}", u.username);
    println!("  Active:     {}", if u.is_active { "yes" } else { "no" });
    println!("  Created At: {}", u.created_at);
    println!("  Updated At: {}", u.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_user(action: UserAction, client: &ApiClient, json_output: bool) -> Result<(), String> {
    match action {
        UserAction::List => {
            if json_output {
                let raw = client.get_raw("/users").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<UserResponse> = client
                    .get("/users")
                    .await?;
                print_users(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        UserAction::Show { user } => {
            if json_output {
                let path = if let Ok(id) = user.parse::<i32>() {
                    format!("/users/{}", id)
                } else {
                    format!("/users_by_name/{}", user)
                };
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: UserResponse = if let Ok(id) = user.parse::<i32>() {
                    client.get(&format!("/users/{}", id)).await?
                } else {
                    client.get(&format!("/users_by_name/{}", user)).await?
                };
                print_user_info(&resp);
            }
        }
        UserAction::Add { username } => {
            let body = CreateUserRequest { username };
            if json_output {
                let raw = client.post_raw("/users", &body).await?;
                println!("{}", raw);
            } else {
                let resp: UserResponse = client
                    .post("/users", &body)
                    .await?;
                print_user_detail(&resp);
            }
        }
        UserAction::Update {
            user,
            username,
            is_active,
        } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = UpdateUserRequestBody {
                username,
                is_active,
            };
            if json_output {
                let raw = client.put_raw(&format!("/users/{}", user_id), &body).await?;
                println!("{}", raw);
            } else {
                let resp: UserResponse = client
                    .put(&format!("/users/{}", user_id), &body)
                    .await?;
                println!("User updated successfully!");
                println!();
                print_user_info(&resp);
            }
        }
    }
    Ok(())
}
