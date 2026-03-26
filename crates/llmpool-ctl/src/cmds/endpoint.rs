use clap::Subcommand;
use serde::Serialize;

use super::{
    EndpointResponse, EndpointWithModelsResponse, PaginatedResponse, TagsResponse,
    TestEndpointResponse, bool_mark, parse_comma_list, print_models, print_pagination,
    resolve_endpoint_id, truncate,
};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum EndpointAction {
    /// List all endpoints
    List,
    /// Test an endpoint (detect features without saving)
    Test {
        /// API key for the endpoint
        #[arg(long)]
        api_key: String,
        /// Base URL of the endpoint
        #[arg(long)]
        api_base: String,
    },
    /// Add a new endpoint
    Add {
        /// Display name for the endpoint
        #[arg(long)]
        name: String,
        /// API key for the endpoint
        #[arg(long)]
        api_key: String,
        /// Base URL of the endpoint
        #[arg(long)]
        api_base: String,
        /// Description of the endpoint
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Comma-separated proxies
        #[arg(long)]
        proxies: Option<String>,
    },
    /// Update an existing endpoint
    Update {
        /// Endpoint name or ID
        #[arg(long)]
        endpoint: String,
        /// New name for the endpoint
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Comma-separated proxies
        #[arg(long)]
        proxies: Option<String>,
        /// Status (online, offline, maintenance)
        #[arg(long)]
        status: Option<String>,
    },
    /// List tags of an endpoint
    Listtags {
        /// Endpoint name or ID
        #[arg(long)]
        endpoint: String,
    },
    /// Add a tag to an endpoint
    Addtag {
        /// Endpoint name or ID
        #[arg(long)]
        endpoint: String,
        /// Tag to add
        #[arg(long)]
        tag: String,
    },
    /// Delete a tag from an endpoint
    Deltag {
        /// Endpoint name or ID
        #[arg(long)]
        endpoint: String,
        /// Tag to delete
        #[arg(long)]
        tag: String,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateEndpointRequest {
    name: String,
    api_key: String,
    api_base: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    proxies: Vec<String>,
}

#[derive(Serialize)]
struct TestEndpointRequest {
    api_key: String,
    api_base: String,
}

#[derive(Serialize)]
struct UpdateEndpointRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

#[derive(Serialize)]
struct AddTagRequestBody {
    tag: String,
}

// ============================================================
// Display Helpers
// ============================================================

fn print_endpoints(endpoints: &[EndpointResponse]) {
    if endpoints.is_empty() {
        println!("No endpoints found.");
        return;
    }

    println!(
        "{:<5} {:<20} {:<40} {:<12} {:<8} {:<20} {:<20}",
        "ID", "Name", "API Base", "Status", "Resp.API", "Tags", "Proxies"
    );
    println!("{}", "-".repeat(125));
    for ep in endpoints {
        println!(
            "{:<5} {:<20} {:<40} {:<12} {:<8} {:<20} {:<20}",
            ep.id,
            truncate(&ep.name, 18),
            truncate(&ep.api_base, 38),
            ep.status,
            if ep.has_responses_api { "yes" } else { "no" },
            truncate(&ep.tags.join(","), 18),
            truncate(&ep.proxies.join(","), 18),
        );
    }
}

fn print_test_result(result: &TestEndpointResponse) {
    println!(
        "Responses API: {}",
        if result.has_responses_api {
            "yes"
        } else {
            "no"
        }
    );
    println!();
    if result.models.is_empty() {
        println!("No models detected.");
        return;
    }
    println!(
        "{:<35} {:<15} {:<6} {:<6} {:<6} {:<6}",
        "Model ID", "Owned By", "Chat", "Embed", "Image", "Speech"
    );
    println!("{}", "-".repeat(80));
    for m in &result.models {
        println!(
            "{:<35} {:<15} {:<6} {:<6} {:<6} {:<6}",
            truncate(&m.model_id, 33),
            truncate(&m.owned_by, 13),
            bool_mark(m.has_chat_completion),
            bool_mark(m.has_embedding),
            bool_mark(m.has_image_generation),
            bool_mark(m.has_speech),
        );
    }
}

fn print_endpoint_with_models(resp: &EndpointWithModelsResponse) {
    println!("Endpoint created successfully!");
    println!();
    println!("  ID:             {}", resp.endpoint.id);
    println!("  Name:           {}", resp.endpoint.name);
    println!("  API Base:       {}", resp.endpoint.api_base);
    println!("  Status:         {}", resp.endpoint.status);
    println!(
        "  Responses API:  {}",
        if resp.endpoint.has_responses_api {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Tags:           {}", resp.endpoint.tags.join(", "));
    println!("  Proxies:        {}", resp.endpoint.proxies.join(", "));
    println!("  Description:    {}", resp.endpoint.description);
    println!();
    if !resp.models.is_empty() {
        println!("Models ({}):", resp.models.len());
        print_models(&resp.models);
    }
}

fn print_endpoint_detail(ep: &EndpointResponse) {
    println!("Endpoint updated successfully!");
    println!();
    println!("  ID:             {}", ep.id);
    println!("  Name:           {}", ep.name);
    println!("  API Base:       {}", ep.api_base);
    println!("  Status:         {}", ep.status);
    println!(
        "  Responses API:  {}",
        if ep.has_responses_api { "yes" } else { "no" }
    );
    println!("  Tags:           {}", ep.tags.join(", "));
    println!("  Proxies:        {}", ep.proxies.join(", "));
    println!("  Description:    {}", ep.description);
    println!("  Created At:     {}", ep.created_at);
    println!("  Updated At:     {}", ep.updated_at);
}

// ============================================================
// Command Handler
// ============================================================

pub async fn handle_endpoint(
    action: EndpointAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        EndpointAction::List => {
            if json_output {
                let raw = client.get_raw("/endpoints").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<EndpointResponse> = client.get("/endpoints").await?;
                print_endpoints(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        EndpointAction::Test { api_key, api_base } => {
            let body = TestEndpointRequest {
                api_key,
                api_base: api_base.clone(),
            };
            if json_output {
                let raw = client.post_raw("/endpoint-tests", &body).await?;
                println!("{}", raw);
            } else {
                println!("Testing endpoint {}...", api_base);
                let resp: TestEndpointResponse = client.post("/endpoint-tests", &body).await?;
                println!();
                print_test_result(&resp);
            }
        }
        EndpointAction::Add {
            name,
            api_key,
            api_base,
            description: _description,
            tags,
            proxies,
        } => {
            let body = CreateEndpointRequest {
                name,
                api_key,
                api_base: api_base.clone(),
                tags: tags.map(|t| parse_comma_list(&t)).unwrap_or_default(),
                proxies: proxies.map(|p| parse_comma_list(&p)).unwrap_or_default(),
            };
            if json_output {
                let raw = client.post_raw("/endpoints", &body).await?;
                println!("{}", raw);
            } else {
                println!("Adding endpoint {}...", api_base);
                let resp: EndpointWithModelsResponse = client.post("/endpoints", &body).await?;
                println!();
                print_endpoint_with_models(&resp);
            }
        }
        EndpointAction::Update {
            endpoint,
            name,
            description,
            tags,
            proxies,
            status,
        } => {
            let endpoint_id = resolve_endpoint_id(&endpoint, client).await?;
            let body = UpdateEndpointRequestBody {
                name,
                tags: tags.map(|t| parse_comma_list(&t)),
                proxies: proxies.map(|p| parse_comma_list(&p)),
                description,
                status,
            };
            if json_output {
                let raw = client
                    .put_raw(&format!("/endpoints/{}", endpoint_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: EndpointResponse = client
                    .put(&format!("/endpoints/{}", endpoint_id), &body)
                    .await?;
                print_endpoint_detail(&resp);
            }
        }
        EndpointAction::Listtags { endpoint } => {
            let endpoint_id = resolve_endpoint_id(&endpoint, client).await?;
            if json_output {
                let raw = client
                    .get_raw(&format!("/endpoints/{}/tags", endpoint_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .get(&format!("/endpoints/{}/tags", endpoint_id))
                    .await?;
                println!("Tags for endpoint {} (ID: {}):", endpoint, resp.endpoint_id);
                if resp.tags.is_empty() {
                    println!("  (no tags)");
                } else {
                    for tag in &resp.tags {
                        println!("  - {}", tag);
                    }
                }
            }
        }
        EndpointAction::Addtag { endpoint, tag } => {
            let endpoint_id = resolve_endpoint_id(&endpoint, client).await?;
            let body = AddTagRequestBody { tag: tag.clone() };
            if json_output {
                let raw = client
                    .post_raw(&format!("/endpoints/{}/tags", endpoint_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .post(&format!("/endpoints/{}/tags", endpoint_id), &body)
                    .await?;
                println!(
                    "Tag '{}' added to endpoint {} (ID: {}).",
                    tag, endpoint, resp.endpoint_id
                );
                println!("Current tags:");
                for t in &resp.tags {
                    println!("  - {}", t);
                }
            }
        }
        EndpointAction::Deltag { endpoint, tag } => {
            let endpoint_id = resolve_endpoint_id(&endpoint, client).await?;
            if json_output {
                let raw = client
                    .delete_raw(&format!("/endpoints/{}/tags/{}", endpoint_id, tag))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .delete(&format!("/endpoints/{}/tags/{}", endpoint_id, tag))
                    .await?;
                println!(
                    "Tag '{}' removed from endpoint {} (ID: {}).",
                    tag, endpoint, resp.endpoint_id
                );
                println!("Current tags:");
                if resp.tags.is_empty() {
                    println!("  (no tags)");
                } else {
                    for t in &resp.tags {
                        println!("  - {}", t);
                    }
                }
            }
        }
    }
    Ok(())
}
