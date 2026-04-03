use clap::Subcommand;
use serde::Serialize;

use super::{
    PaginatedResponse, TagsResponse, TestUpstreamResponse, UpstreamResponse,
    UpstreamWithModelsResponse, bool_mark, parse_comma_list, print_models, print_pagination,
    resolve_upstream_id, truncate,
};
use crate::client::ApiClient;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Subcommand)]
pub enum UpstreamAction {
    /// List all upstreams
    List,
    /// Test an upstream (detect features without saving)
    Test {
        /// API key for the upstream
        #[arg(long)]
        api_key: String,
        /// Base URL of the upstream
        #[arg(long)]
        api_base: String,
    },
    /// Add a new upstream
    Add {
        /// Display name for the upstream
        #[arg(long)]
        name: String,
        /// API key for the upstream
        #[arg(long)]
        api_key: String,
        /// Base URL of the upstream
        #[arg(long)]
        api_base: String,
        /// Provider type (openai, azure, cohere, anthropic, vllm, ollama)
        #[arg(long, default_value = "openai")]
        provider: String,
        /// Description of the upstream
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Comma-separated proxies
        #[arg(long)]
        proxies: Option<String>,
        /// Probe each model for supported features (chat, embedding, image, speech).
        /// When omitted (default), models are saved with all feature flags set to false
        /// without making any additional requests to the upstream.
        #[arg(long, default_value_t = false)]
        detect: bool,
    },
    /// Update an existing upstream
    Update {
        /// Upstream name or ID
        #[arg(long)]
        upstream: String,
        /// New name for the upstream
        #[arg(long)]
        name: Option<String>,
        /// Provider type (openai, azure, cohere, anthropic, vllm, ollama)
        #[arg(long)]
        provider: Option<String>,
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
    /// List tags of an upstream
    Listtags {
        /// Upstream name or ID
        #[arg(long)]
        upstream: String,
    },
    /// Add a tag to an upstream
    Addtag {
        /// Upstream name or ID
        #[arg(long)]
        upstream: String,
        /// Tag to add
        #[arg(long)]
        tag: String,
    },
    /// Delete a tag from an upstream
    Deltag {
        /// Upstream name or ID
        #[arg(long)]
        upstream: String,
        /// Tag to delete
        #[arg(long)]
        tag: String,
    },
}

// ============================================================
// Request Types
// ============================================================

#[derive(Serialize)]
struct CreateUpstreamRequest {
    name: String,
    api_key: String,
    api_base: String,
    provider: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    proxies: Vec<String>,
    detect: bool,
}

#[derive(Serialize)]
struct TestUpstreamRequest {
    api_key: String,
    api_base: String,
}

#[derive(Serialize)]
struct UpdateUpstreamRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
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

fn print_upstreams(upstreams: &[UpstreamResponse]) {
    if upstreams.is_empty() {
        println!("No upstreams found.");
        return;
    }

    println!(
        "{:<5} {:<20} {:<40} {:<12} {:<8} {:<20} {:<20}",
        "ID", "Name", "API Base", "Status", "Resp.API", "Tags", "Proxies"
    );
    println!("{}", "-".repeat(125));
    for ep in upstreams {
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

fn print_test_result(result: &TestUpstreamResponse) {
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

fn print_upstream_with_models(resp: &UpstreamWithModelsResponse) {
    println!("Upstream created successfully!");
    println!();
    println!("  ID:             {}", resp.upstream.id);
    println!("  Name:           {}", resp.upstream.name);
    println!("  API Base:       {}", resp.upstream.api_base);
    println!("  Provider:       {}", resp.upstream.provider);
    println!("  Status:         {}", resp.upstream.status);
    println!(
        "  Responses API:  {}",
        if resp.upstream.has_responses_api {
            "yes"
        } else {
            "no"
        }
    );
    println!("  Tags:           {}", resp.upstream.tags.join(", "));
    println!("  Proxies:        {}", resp.upstream.proxies.join(", "));
    println!("  Description:    {}", resp.upstream.description);
    println!();
    if !resp.models.is_empty() {
        println!("Models ({}):", resp.models.len());
        print_models(&resp.models);
    }
}

fn print_upstream_detail(ep: &UpstreamResponse) {
    println!("Upstream updated successfully!");
    println!();
    println!("  ID:             {}", ep.id);
    println!("  Name:           {}", ep.name);
    println!("  API Base:       {}", ep.api_base);
    println!("  Provider:       {}", ep.provider);
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

pub async fn handle_upstream(
    action: UpstreamAction,
    client: &ApiClient,
    json_output: bool,
) -> Result<(), String> {
    match action {
        UpstreamAction::List => {
            if json_output {
                let raw = client.get_raw("/upstreams").await?;
                println!("{}", raw);
            } else {
                let resp: PaginatedResponse<UpstreamResponse> = client.get("/upstreams").await?;
                print_upstreams(&resp.data);
                print_pagination(&resp.pagination);
            }
        }
        UpstreamAction::Test { api_key, api_base } => {
            let body = TestUpstreamRequest {
                api_key,
                api_base: api_base.clone(),
            };
            if json_output {
                let raw = client.post_raw("/upstream-tests", &body).await?;
                println!("{}", raw);
            } else {
                println!("Testing upstream {}...", api_base);
                let resp: TestUpstreamResponse = client.post("/upstream-tests", &body).await?;
                println!();
                print_test_result(&resp);
            }
        }
        UpstreamAction::Add {
            name,
            api_key,
            api_base,
            provider,
            description: _description,
            tags,
            proxies,
            detect,
        } => {
            let body = CreateUpstreamRequest {
                name,
                api_key,
                api_base: api_base.clone(),
                provider,
                tags: tags.map(|t| parse_comma_list(&t)).unwrap_or_default(),
                proxies: proxies.map(|p| parse_comma_list(&p)).unwrap_or_default(),
                detect,
            };
            if json_output {
                let raw = client.post_raw("/upstreams", &body).await?;
                println!("{}", raw);
            } else {
                println!("Adding upstream {}...", api_base);
                let resp: UpstreamWithModelsResponse = client.post("/upstreams", &body).await?;
                println!();
                print_upstream_with_models(&resp);
            }
        }
        UpstreamAction::Update {
            upstream,
            name,
            provider,
            description,
            tags,
            proxies,
            status,
        } => {
            let upstream_id = resolve_upstream_id(&upstream, client).await?;
            let body = UpdateUpstreamRequestBody {
                name,
                provider,
                tags: tags.map(|t| parse_comma_list(&t)),
                proxies: proxies.map(|p| parse_comma_list(&p)),
                description,
                status,
            };
            if json_output {
                let raw = client
                    .put_raw(&format!("/upstreams/{}", upstream_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: UpstreamResponse = client
                    .put(&format!("/upstreams/{}", upstream_id), &body)
                    .await?;
                print_upstream_detail(&resp);
            }
        }
        UpstreamAction::Listtags { upstream } => {
            let upstream_id = resolve_upstream_id(&upstream, client).await?;
            if json_output {
                let raw = client
                    .get_raw(&format!("/upstreams/{}/tags", upstream_id))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .get(&format!("/upstreams/{}/tags", upstream_id))
                    .await?;
                println!("Tags for upstream {} (ID: {}):", upstream, resp.upstream_id);
                if resp.tags.is_empty() {
                    println!("  (no tags)");
                } else {
                    for tag in &resp.tags {
                        println!("  - {}", tag);
                    }
                }
            }
        }
        UpstreamAction::Addtag { upstream, tag } => {
            let upstream_id = resolve_upstream_id(&upstream, client).await?;
            let body = AddTagRequestBody { tag: tag.clone() };
            if json_output {
                let raw = client
                    .post_raw(&format!("/upstreams/{}/tags", upstream_id), &body)
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .post(&format!("/upstreams/{}/tags", upstream_id), &body)
                    .await?;
                println!(
                    "Tag '{}' added to upstream {} (ID: {}).",
                    tag, upstream, resp.upstream_id
                );
                println!("Current tags:");
                for t in &resp.tags {
                    println!("  - {}", t);
                }
            }
        }
        UpstreamAction::Deltag { upstream, tag } => {
            let upstream_id = resolve_upstream_id(&upstream, client).await?;
            if json_output {
                let raw = client
                    .delete_raw(&format!("/upstreams/{}/tags/{}", upstream_id, tag))
                    .await?;
                println!("{}", raw);
            } else {
                let resp: TagsResponse = client
                    .delete(&format!("/upstreams/{}/tags/{}", upstream_id, tag))
                    .await?;
                println!(
                    "Tag '{}' removed from upstream {} (ID: {}).",
                    tag, upstream, resp.upstream_id
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
