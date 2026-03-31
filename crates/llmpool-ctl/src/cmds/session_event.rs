use clap::Subcommand;
use serde::Deserialize;

use super::{CursorResponse, truncate};
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
        /// Start cursor (event ID to start after, exclusive)
        #[arg(long, default_value = "0")]
        start: i64,
        /// Number of items to return
        #[arg(long, default_value = "20")]
        count: i64,
    },
    /// Get a session event by ID
    Get {
        /// The session event ID
        event_id: i64,
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
    pub account_id: i32,
    pub model_id: i32,
    pub api_key_id: i32,
    pub input_token_price: String,
    pub input_tokens: i64,
    pub output_token_price: String,
    pub output_tokens: i64,
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
        "{:<8} {:<38} {:<6} {:<10} {:<8} {:<8} {:<12} {:<12} {:<22}",
        "ID",
        "Session ID",
        "Index",
        "Account",
        "Model",
        "APIKey",
        "InTokens",
        "OutTokens",
        "Created At"
    );
    println!("{}", "-".repeat(124));
    for e in events {
        println!(
            "{:<8} {:<38} {:<6} {:<10} {:<8} {:<8} {:<12} {:<12} {:<22}",
            e.id,
            truncate(&e.session_id, 36),
            e.session_index,
            e.account_id,
            e.model_id,
            e.api_key_id,
            e.input_tokens,
            e.output_tokens,
            e.created_at,
        );
    }
}

fn print_session_event_detail(e: &SessionEventResponse) {
    println!("ID:                {}", e.id);
    println!("Session ID:        {}", e.session_id);
    println!("Session Index:     {}", e.session_index);
    println!("Account ID:        {}", e.account_id);
    println!("Model ID:          {}", e.model_id);
    println!("API Key ID:        {}", e.api_key_id);
    println!("Input Token Price: {}", e.input_token_price);
    println!("Input Tokens:      {}", e.input_tokens);
    println!("Output Token Price:{}", e.output_token_price);
    println!("Output Tokens:     {}", e.output_tokens);
    println!("Created At:        {}", e.created_at);
    println!(
        "Event Data:        {}",
        serde_json::to_string_pretty(&e.event_data).unwrap_or_else(|_| e.event_data.to_string())
    );
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
        SessionEventAction::List {
            session,
            start,
            count,
        } => {
            let mut path = format!("/session-events?start={}&count={}", start, count);
            if let Some(ref sid) = session {
                path = format!("{}&session={}", path, sid);
            }

            if json_output {
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let resp: CursorResponse<SessionEventResponse> = client.get(&path).await?;
                print_session_events(&resp.data);
                if resp.has_more {
                    println!("\nhas_more: true, next_id: {}", resp.next_id);
                }
            }
        }
        SessionEventAction::Get { event_id } => {
            let path = format!("/session-events/{}", event_id);

            if json_output {
                let raw = client.get_raw(&path).await?;
                println!("{}", raw);
            } else {
                let event: SessionEventResponse = client.get(&path).await?;
                print_session_event_detail(&event);
            }
        }
    }
    Ok(())
}
