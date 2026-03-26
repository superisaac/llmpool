use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::process;

mod client;

// ============================================================
// CLI Definitions
// ============================================================

#[derive(Parser)]
#[command(name = "llmpool-ctl", about = "CLI tool for managing LLMPool via Admin API")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage endpoints
    Endpoint {
        #[command(subcommand)]
        action: EndpointAction,
    },
    /// Manage models
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Manage users
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Manage API keys
    Apikey {
        #[command(subcommand)]
        action: ApiKeyAction,
    },
    /// Manage user funds (balance, deposit, withdraw, credit)
    Fund {
        #[command(subcommand)]
        action: FundAction,
    },
}

#[derive(Subcommand)]
enum EndpointAction {
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
        /// ID of the endpoint to update
        #[arg(long)]
        endpoint_id: i32,
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
}

#[derive(Subcommand)]
enum ModelAction {
    /// List all models
    List,
    /// Update a model
    Update {
        /// ID of the model to update
        #[arg(long)]
        model_id: i32,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
}

#[derive(Subcommand)]
enum UserAction {
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

#[derive(Subcommand)]
enum ApiKeyAction {
    /// List API keys for a user
    List {
        /// Username or user ID
        #[arg(long)]
        user: String,
    },
    /// Add a new API key for a user
    Add {
        /// Username or user ID
        #[arg(long)]
        user: String,
        /// Label describing the purpose of this API key
        #[arg(long, default_value = "")]
        label: String,
    },
}

#[derive(Subcommand)]
enum FundAction {
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
// API Response Types
// ============================================================

#[derive(Debug, Deserialize)]
struct PaginatedResponse<T> {
    data: Vec<T>,
    pagination: PaginationInfo,
}

#[derive(Debug, Deserialize)]
struct PaginationInfo {
    page: i64,
    page_size: i64,
    total: i64,
    total_pages: i64,
}

#[derive(Debug, Deserialize)]
struct EndpointResponse {
    id: i32,
    name: String,
    api_base: String,
    has_responses_api: bool,
    tags: Vec<String>,
    proxies: Vec<String>,
    status: String,
    description: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ModelResponse {
    id: i32,
    endpoint_id: i32,
    model_id: String,
    has_chat_completion: bool,
    has_embedding: bool,
    has_image_generation: bool,
    has_speech: bool,
    description: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct UserResponse {
    id: i32,
    username: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FundResponse {
    id: i32,
    user_id: i32,
    cash: String,
    credit: String,
    debt: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct BalanceChangeResponse {
    id: i32,
    user_id: i32,
    unique_request_id: String,
    content: serde_json::Value,
    is_applied: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct EndpointWithModelsResponse {
    endpoint: EndpointResponse,
    models: Vec<ModelResponse>,
}

#[derive(Debug, Deserialize)]
struct ModelFeaturesResponse {
    model_id: String,
    owned_by: String,
    has_chat_completion: bool,
    has_embedding: bool,
    has_image_generation: bool,
    has_speech: bool,
}

#[derive(Debug, Deserialize)]
struct TestEndpointResponse {
    has_responses_api: bool,
    models: Vec<ModelFeaturesResponse>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAIAPIKeyResponse {
    id: i32,
    user_id: Option<i32>,
    apikey: String,
    label: String,
    is_active: bool,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
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
struct UpdateModelRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Serialize)]
struct CreateUserRequest {
    username: String,
}

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

#[derive(Serialize)]
struct CreateApiKeyRequestBody {
    label: String,
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

fn print_models(models: &[ModelResponse]) {
    if models.is_empty() {
        println!("No models found.");
        return;
    }

    println!(
        "{:<5} {:<10} {:<35} {:<6} {:<6} {:<6} {:<6} {:<30}",
        "ID", "EP ID", "Model ID", "Chat", "Embed", "Image", "Speech", "Description"
    );
    println!("{}", "-".repeat(110));
    for m in models {
        println!(
            "{:<5} {:<10} {:<35} {:<6} {:<6} {:<6} {:<6} {:<30}",
            m.id,
            m.endpoint_id,
            truncate(&m.model_id, 33),
            bool_mark(m.has_chat_completion),
            bool_mark(m.has_embedding),
            bool_mark(m.has_image_generation),
            bool_mark(m.has_speech),
            truncate(&m.description, 28),
        );
    }
}

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

fn print_apikeys(keys: &[OpenAIAPIKeyResponse]) {
    if keys.is_empty() {
        println!("No API keys found.");
        return;
    }

    println!(
        "{:<5} {:<38} {:<20} {:<8} {:<22}",
        "ID", "API Key", "Label", "Active", "Created At"
    );
    println!("{}", "-".repeat(95));
    for ak in keys {
        println!(
            "{:<5} {:<38} {:<20} {:<8} {:<22}",
            ak.id,
            truncate(&ak.apikey, 36),
            truncate(&ak.label, 18),
            if ak.is_active { "yes" } else { "no" },
            ak.created_at,
        );
    }
}

fn print_apikey_detail(ak: &OpenAIAPIKeyResponse) {
    println!("API key created successfully!");
    println!();
    println!("  ID:         {}", ak.id);
    println!("  API Key:    {}", ak.apikey);
    println!("  Label:      {}", ak.label);
    println!("  Active:     {}", if ak.is_active { "yes" } else { "no" });
    if let Some(ref expires) = ak.expires_at {
        println!("  Expires At: {}", expires);
    }
    println!("  Created At: {}", ak.created_at);
    println!("  Updated At: {}", ak.updated_at);
}

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

fn print_test_result(result: &TestEndpointResponse) {
    println!("Responses API: {}", if result.has_responses_api { "yes" } else { "no" });
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
    println!("  Responses API:  {}", if resp.endpoint.has_responses_api { "yes" } else { "no" });
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
    println!("  Responses API:  {}", if ep.has_responses_api { "yes" } else { "no" });
    println!("  Tags:           {}", ep.tags.join(", "));
    println!("  Proxies:        {}", ep.proxies.join(", "));
    println!("  Description:    {}", ep.description);
    println!("  Created At:     {}", ep.created_at);
    println!("  Updated At:     {}", ep.updated_at);
}

fn print_model_detail(m: &ModelResponse) {
    println!("Model updated successfully!");
    println!();
    println!("  ID:               {}", m.id);
    println!("  Endpoint ID:      {}", m.endpoint_id);
    println!("  Model ID:         {}", m.model_id);
    println!("  Chat Completion:  {}", bool_mark(m.has_chat_completion));
    println!("  Embedding:        {}", bool_mark(m.has_embedding));
    println!("  Image Generation: {}", bool_mark(m.has_image_generation));
    println!("  Speech:           {}", bool_mark(m.has_speech));
    println!("  Description:      {}", m.description);
    println!("  Created At:       {}", m.created_at);
    println!("  Updated At:       {}", m.updated_at);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}…", &s[..max_len - 1])
    } else {
        s.to_string()
    }
}

fn bool_mark(v: bool) -> &'static str {
    if v { "✓" } else { "✗" }
}

fn parse_comma_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn print_pagination(pagination: &PaginationInfo) {
    if pagination.total_pages > 1 {
        println!(
            "\nPage {}/{} (total: {}, page_size: {})",
            pagination.page, pagination.total_pages, pagination.total, pagination.page_size
        );
    } else {
        println!("\nTotal: {}", pagination.total);
    }
}

// ============================================================
// User ID Resolution
// ============================================================

/// Resolve a username or user ID string to a numeric user ID.
/// If the string can be parsed as an integer, it is used directly as a user ID.
/// Otherwise, it is treated as a username and looked up via /users_by_name/{username}.
async fn resolve_user_id(user: &str, client: &client::ApiClient) -> Result<i32, String> {
    if let Ok(id) = user.parse::<i32>() {
        return Ok(id);
    }
    // Treat as username, look up via API
    let resp: UserResponse = client
        .get(&format!("/users_by_name/{}", user))
        .await?;
    Ok(resp.id)
}

// ============================================================
// Main
// ============================================================

#[tokio::main]
async fn main() {
    // Load .env file if present (ignore errors)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let admin_url = match std::env::var("LLMPOOL_ADMIN_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: LLMPOOL_ADMIN_URL environment variable is not set.");
            eprintln!("Set it directly or add it to a .env file in the current directory.");
            process::exit(1);
        }
    };

    let admin_token = match std::env::var("LLMPOOL_ADMIN_TOKEN") {
        Ok(token) => token,
        Err(_) => {
            eprintln!("Error: LLMPOOL_ADMIN_TOKEN environment variable is not set.");
            eprintln!("Set it directly or add it to a .env file in the current directory.");
            process::exit(1);
        }
    };

    let api_client = client::ApiClient::new(admin_url, admin_token);

    let result = match cli.command {
        Commands::Endpoint { action } => handle_endpoint(action, &api_client).await,
        Commands::Model { action } => handle_model(action, &api_client).await,
        Commands::User { action } => handle_user(action, &api_client).await,
        Commands::Apikey { action } => handle_apikey(action, &api_client).await,
        Commands::Fund { action } => handle_fund(action, &api_client).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

// ============================================================
// Command Handlers
// ============================================================

async fn handle_endpoint(action: EndpointAction, client: &client::ApiClient) -> Result<(), String> {
    match action {
        EndpointAction::List => {
            let resp: PaginatedResponse<EndpointResponse> = client
                .get("/endpoints")
                .await?;
            print_endpoints(&resp.data);
            print_pagination(&resp.pagination);
        }
        EndpointAction::Test { api_key, api_base } => {
            println!("Testing endpoint {}...", api_base);
            let body = TestEndpointRequest { api_key, api_base };
            let resp: TestEndpointResponse = client
                .post("/endpoint-tests", &body)
                .await?;
            println!();
            print_test_result(&resp);
        }
        EndpointAction::Add {
            name,
            api_key,
            api_base,
            description: _description,
            tags,
            proxies,
        } => {
            println!("Adding endpoint {}...", api_base);
            let body = CreateEndpointRequest {
                name,
                api_key,
                api_base,
                tags: tags.map(|t| parse_comma_list(&t)).unwrap_or_default(),
                proxies: proxies.map(|p| parse_comma_list(&p)).unwrap_or_default(),
            };
            let resp: EndpointWithModelsResponse = client
                .post("/endpoints", &body)
                .await?;
            println!();
            print_endpoint_with_models(&resp);
        }
        EndpointAction::Update {
            endpoint_id,
            name,
            description,
            tags,
            proxies,
            status,
        } => {
            let body = UpdateEndpointRequestBody {
                name,
                tags: tags.map(|t| parse_comma_list(&t)),
                proxies: proxies.map(|p| parse_comma_list(&p)),
                description,
                status,
            };
            let resp: EndpointResponse = client
                .put(&format!("/endpoints/{}", endpoint_id), &body)
                .await?;
            print_endpoint_detail(&resp);
        }
    }
    Ok(())
}

async fn handle_model(action: ModelAction, client: &client::ApiClient) -> Result<(), String> {
    match action {
        ModelAction::List => {
            let resp: PaginatedResponse<ModelResponse> = client
                .get("/models")
                .await?;
            print_models(&resp.data);
            print_pagination(&resp.pagination);
        }
        ModelAction::Update {
            model_id,
            description,
        } => {
            let body = UpdateModelRequestBody { description };
            let resp: ModelResponse = client
                .put(&format!("/models/{}", model_id), &body)
                .await?;
            print_model_detail(&resp);
        }
    }
    Ok(())
}

async fn handle_user(action: UserAction, client: &client::ApiClient) -> Result<(), String> {
    match action {
        UserAction::List => {
            let resp: PaginatedResponse<UserResponse> = client
                .get("/users")
                .await?;
            print_users(&resp.data);
            print_pagination(&resp.pagination);
        }
        UserAction::Show { user } => {
            let resp: UserResponse = if let Ok(id) = user.parse::<i32>() {
                client.get(&format!("/users/{}", id)).await?
            } else {
                client.get(&format!("/users_by_name/{}", user)).await?
            };
            print_user_info(&resp);
        }
        UserAction::Add { username } => {
            let body = CreateUserRequest { username };
            let resp: UserResponse = client
                .post("/users", &body)
                .await?;
            print_user_detail(&resp);
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
            let resp: UserResponse = client
                .put(&format!("/users/{}", user_id), &body)
                .await?;
            println!("User updated successfully!");
            println!();
            print_user_info(&resp);
        }
    }
    Ok(())
}

async fn handle_apikey(action: ApiKeyAction, client: &client::ApiClient) -> Result<(), String> {
    match action {
        ApiKeyAction::List { user } => {
            let user_id = resolve_user_id(&user, client).await?;
            let resp: PaginatedResponse<OpenAIAPIKeyResponse> = client
                .get(&format!("/users/{}/apikeys", user_id))
                .await?;
            print_apikeys(&resp.data);
            print_pagination(&resp.pagination);
        }
        ApiKeyAction::Add { user, label } => {
            let user_id = resolve_user_id(&user, client).await?;
            let body = CreateApiKeyRequestBody { label };
            let resp: OpenAIAPIKeyResponse = client
                .post(&format!("/users/{}/apikeys", user_id), &body)
                .await?;
            print_apikey_detail(&resp);
        }
    }
    Ok(())
}

async fn handle_fund(action: FundAction, client: &client::ApiClient) -> Result<(), String> {
    match action {
        FundAction::Show { user } => {
            let user_id = resolve_user_id(&user, client).await?;
            let resp: FundResponse = client
                .get(&format!("/users/{}/fund", user_id))
                .await?;
            print_fund_detail(&resp);
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
            let resp: BalanceChangeResponse = client
                .post("/deposits", &body)
                .await?;
            print_balance_change(&resp, "Deposit");
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
            let resp: BalanceChangeResponse = client
                .post("/withdrawals", &body)
                .await?;
            print_balance_change(&resp, "Withdrawal");
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
            let resp: BalanceChangeResponse = client
                .post("/credits", &body)
                .await?;
            print_balance_change(&resp, "Credit");
        }
    }
    Ok(())
}
