use clap::Subcommand;
use serde::Deserialize;

use super::{PaginatedResponse, print_pagination, truncate};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum SessionEventAction {
    /// List session events
    List {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
    },
}

// ============================================================
// Response Types
// ============================================================

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SessionEventResponse {
    pub id: i64,
    pub session_id: String,
    pub session_index: i32,
    pub consumer_id: i32,
    pub model_id: i32,
    pub api_key_id: i32,
    pub event_data: serde_json::Value,
    pub created_at: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_session_events(events: &[SessionEventResponse]) {
    if events.is_empty() {
        println!("No session events found.");
        return;
    }

    println!(
        "{:<8} {:<38} {:<6} {:<10} {:<8} {:<8} {:<22}",
        "ID", "Session ID", "Index", "Consumer", "Model", "APIKey", "Created At"
    );
    println!("{}", "-".repeat(100));
    for e in events {
        println!(
            "{:<8} {:<38} {:<6} {:<10} {:<8} {:<8} {:<22}",
            e.id,
            truncate(&e.session_id, 36),
            e.session_index,
            e.consumer_id,
            e.model_id,
            e.api_key_id,
            e.created_at,
        );
    }
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_session_event(
    action: SessionEventAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        SessionEventAction::List { session } => {
            let path = if let Some(ref sid) = session {
                format!("/sessionevents?session={}", sid)
            } else {
                "/sessionevents".to_string()
            };

            if json_output {
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<SessionEventResponse> = client.get(&path).await?;
                print_session_events(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
    }
    Ok(())
}
