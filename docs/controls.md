# llmpool-ctl — CLI Management Tool

`llmpool-ctl` is a command-line tool for managing LLMPool via the Admin REST API. It provides a convenient interface for managing upstreams, models, accounts, API keys, and account funds without needing to make raw HTTP requests.

## Prerequisites

### Environment Variables

`llmpool-ctl` requires two environment variables to connect to the LLMPool Admin API:

| Variable | Description |
|----------|-------------|
| `LLMPOOL_ADMIN_URL` | Base URL of the LLMPool server (e.g., `http://localhost:19324`) |
| `LLMPOOL_ADMIN_TOKEN` | Admin JWT token for authentication |

You can set these directly or add them to a `.env` file in the current directory (automatically loaded via `dotenvy`).

```bash
# .env file example
LLMPOOL_ADMIN_URL=http://localhost:19324
LLMPOOL_ADMIN_TOKEN=your-admin-jwt-token
```

To generate an admin JWT token, use:

```bash
llmpool admin create-jwt-token
```

### Build

```bash
cargo build --release -p llmpool-ctl
# Binary: target/release/llmpool-ctl
```

## Global Options

| Option | Description |
|--------|-------------|
| `--format json` | Output results in JSON format instead of human-readable tables |

## Commands

### Upstream Management

Manage OpenAI-compatible backend upstreams.

#### `upstream list`

List all registered upstreams.

```bash
llmpool-ctl upstream list
```

#### `upstream test`

Test an upstream by detecting its capabilities and available models without saving to the database.

```bash
llmpool-ctl upstream test --api-key sk-xxx --api-base https://api.openai.com/v1
```

| Flag | Required | Description |
|------|----------|-------------|
| `--api-key` | Yes | API key for the upstream |
| `--api-base` | Yes | Base URL of the upstream |

#### `upstream add`

Add a new upstream. This will auto-detect supported models and capabilities.

```bash
llmpool-ctl upstream add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1 \
  --tags "production,openai" \
  --proxies "http://proxy:8080"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--name` | Yes | Display name for the upstream |
| `--api-key` | Yes | API key for the upstream |
| `--api-base` | Yes | Base URL of the upstream |
| `--description` | No | Description of the upstream |
| `--tags` | No | Comma-separated tags |
| `--proxies` | No | Comma-separated proxy URLs |

#### `upstream update`

Update an existing upstream's properties.

```bash
llmpool-ctl upstream update \
  --upstream "OpenAI" \
  --description "Main OpenAI upstream" \
  --status online
```

| Flag | Required | Description |
|------|----------|-------------|
| `--upstream` | Yes | Upstream name or numeric ID |
| `--name` | No | New name |
| `--description` | No | New description |
| `--tags` | No | Comma-separated tags (replaces existing) |
| `--proxies` | No | Comma-separated proxy URLs (replaces existing) |
| `--status` | No | Status: `online`, `offline`, or `maintenance` |

#### `upstream listtags`

List tags of an upstream.

```bash
llmpool-ctl upstream listtags --upstream "OpenAI"
```

#### `upstream addtag`

Add a tag to an upstream.

```bash
llmpool-ctl upstream addtag --upstream "OpenAI" --tag "production"
```

#### `upstream deltag`

Remove a tag from an upstream.

```bash
llmpool-ctl upstream deltag --upstream "OpenAI" --tag "deprecated"
```

---

### Model Management

Manage models associated with upstreams.

#### `model list`

List all models across all upstreams.

```bash
llmpool-ctl model list
```

#### `model update`

Update a model's metadata (description, pricing).

```bash
llmpool-ctl model update \
  --model-id 1 \
  --description "GPT-4o model" \
  --input-token-price "0.0025" \
  --output-token-price "0.01"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--model-id` | Yes | Numeric ID of the model |
| `--description` | No | New description |
| `--input-token-price` | No | Price per input token |
| `--output-token-price` | No | Price per output token |

---

### Account Management

Manage accounts.

#### `account list`

List all accounts.

```bash
llmpool-ctl account list
```

#### `account show`

Show details of a specific account.

```bash
llmpool-ctl account show --account alice
llmpool-ctl account show --account 1
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |

#### `account add`

Create a new account.

```bash
llmpool-ctl account add alice
```

The positional argument is the name.

#### `account update`

Update an existing account.

```bash
llmpool-ctl account update --account alice --name alice2 --is-active false
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |
| `--name` | No | New name |
| `--is-active` | No | Whether the account is active (`true`/`false`) |

---

### API Key Management

Manage OpenAI-compatible API keys for accounts.

#### `apikey list`

List all API keys for an account.

```bash
llmpool-ctl apikey list --account alice
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |

#### `apikey add`

Create a new API key for an account.

```bash
llmpool-ctl apikey add --account alice --label "dev key"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |
| `--label` | No | Label describing the purpose of this API key |

---

### Fund Management

Manage account balances — view balance, deposit cash, withdraw cash, and add credit.

#### `fund show`

Show an account's fund balance (cash, credit, debt).

```bash
llmpool-ctl fund show --account alice
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |

#### `fund deposit`

Deposit cash to an account's fund.

```bash
llmpool-ctl fund deposit --account alice --amount "100.00" --request-id "dep-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |
| `--amount` | Yes | Amount to deposit |
| `--request-id` | Yes | Unique request ID for idempotency |

#### `fund withdraw`

Withdraw cash from an account's fund.

```bash
llmpool-ctl fund withdraw --account alice --amount "50.00" --request-id "wd-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |
| `--amount` | Yes | Amount to withdraw |
| `--request-id` | Yes | Unique request ID for idempotency |

#### `fund credit`

Add credit to an account's fund.

```bash
llmpool-ctl fund credit --account alice --amount "200.00" --request-id "cr-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--account` | Yes | Account name or numeric account ID |
| `--amount` | Yes | Amount of credit to add |
| `--request-id` | Yes | Unique request ID for idempotency |

---

## JSON Output

All commands support `--format json` for machine-readable output, useful for scripting and automation:

```bash
llmpool-ctl --format json upstream list
llmpool-ctl --format json account list
llmpool-ctl --format json fund show --account alice
```

## Name or ID Resolution

For `--upstream` and `--account` flags, you can use either the name (string) or the numeric ID. The tool will automatically resolve names to IDs via the API.

```bash
# Both are equivalent
llmpool-ctl account show --account alice
llmpool-ctl account show --account 1
```

## Example Workflow

```bash
# Set up environment
export LLMPOOL_ADMIN_URL=http://localhost:19324
export LLMPOOL_ADMIN_TOKEN=$(llmpool admin create-jwt-token)

# Add an upstream
llmpool-ctl upstream add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1

# List detected models
llmpool-ctl model list

# Create an account and API key
llmpool-ctl account add alice
llmpool-ctl apikey add --account alice --label "development"

# Deposit funds
llmpool-ctl fund deposit --account alice --amount "100.00" --request-id "initial-deposit"

# Check balance
llmpool-ctl fund show --account alice
```
