# llmpool-ctl — CLI Management Tool

`llmpool-ctl` is a command-line tool for managing LLMPool via the Admin REST API. It provides a convenient interface for managing endpoints, models, consumers, API keys, and consumer funds without needing to make raw HTTP requests.

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

### Endpoint Management

Manage OpenAI-compatible backend endpoints.

#### `endpoint list`

List all registered endpoints.

```bash
llmpool-ctl endpoint list
```

#### `endpoint test`

Test an endpoint by detecting its capabilities and available models without saving to the database.

```bash
llmpool-ctl endpoint test --api-key sk-xxx --api-base https://api.openai.com/v1
```

| Flag | Required | Description |
|------|----------|-------------|
| `--api-key` | Yes | API key for the endpoint |
| `--api-base` | Yes | Base URL of the endpoint |

#### `endpoint add`

Add a new endpoint. This will auto-detect supported models and capabilities.

```bash
llmpool-ctl endpoint add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1 \
  --tags "production,openai" \
  --proxies "http://proxy:8080"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--name` | Yes | Display name for the endpoint |
| `--api-key` | Yes | API key for the endpoint |
| `--api-base` | Yes | Base URL of the endpoint |
| `--description` | No | Description of the endpoint |
| `--tags` | No | Comma-separated tags |
| `--proxies` | No | Comma-separated proxy URLs |

#### `endpoint update`

Update an existing endpoint's properties.

```bash
llmpool-ctl endpoint update \
  --endpoint "OpenAI" \
  --description "Main OpenAI endpoint" \
  --status online
```

| Flag | Required | Description |
|------|----------|-------------|
| `--endpoint` | Yes | Endpoint name or numeric ID |
| `--name` | No | New name |
| `--description` | No | New description |
| `--tags` | No | Comma-separated tags (replaces existing) |
| `--proxies` | No | Comma-separated proxy URLs (replaces existing) |
| `--status` | No | Status: `online`, `offline`, or `maintenance` |

#### `endpoint listtags`

List tags of an endpoint.

```bash
llmpool-ctl endpoint listtags --endpoint "OpenAI"
```

#### `endpoint addtag`

Add a tag to an endpoint.

```bash
llmpool-ctl endpoint addtag --endpoint "OpenAI" --tag "production"
```

#### `endpoint deltag`

Remove a tag from an endpoint.

```bash
llmpool-ctl endpoint deltag --endpoint "OpenAI" --tag "deprecated"
```

---

### Model Management

Manage models associated with endpoints.

#### `model list`

List all models across all endpoints.

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

### Consumer Management

Manage consumers.

#### `consumer list`

List all consumers.

```bash
llmpool-ctl consumer list
```

#### `consumer show`

Show details of a specific consumer.

```bash
llmpool-ctl consumer show --consumer alice
llmpool-ctl consumer show --consumer 1
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |

#### `consumer add`

Create a new consumer.

```bash
llmpool-ctl consumer add alice
```

The positional argument is the name.

#### `consumer update`

Update an existing consumer.

```bash
llmpool-ctl consumer update --consumer alice --name alice2 --is-active false
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |
| `--name` | No | New name |
| `--is-active` | No | Whether the consumer is active (`true`/`false`) |

---

### API Key Management

Manage OpenAI-compatible API keys for consumers.

#### `apikey list`

List all API keys for a consumer.

```bash
llmpool-ctl apikey list --consumer alice
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |

#### `apikey add`

Create a new API key for a consumer.

```bash
llmpool-ctl apikey add --consumer alice --label "dev key"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |
| `--label` | No | Label describing the purpose of this API key |

---

### Fund Management

Manage consumer balances — view balance, deposit cash, withdraw cash, and add credit.

#### `fund show`

Show a consumer's fund balance (cash, credit, debt).

```bash
llmpool-ctl fund show --consumer alice
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |

#### `fund deposit`

Deposit cash to a consumer's fund.

```bash
llmpool-ctl fund deposit --consumer alice --amount "100.00" --request-id "dep-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |
| `--amount` | Yes | Amount to deposit |
| `--request-id` | Yes | Unique request ID for idempotency |

#### `fund withdraw`

Withdraw cash from a consumer's fund.

```bash
llmpool-ctl fund withdraw --consumer alice --amount "50.00" --request-id "wd-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |
| `--amount` | Yes | Amount to withdraw |
| `--request-id` | Yes | Unique request ID for idempotency |

#### `fund credit`

Add credit to a consumer's fund.

```bash
llmpool-ctl fund credit --consumer alice --amount "200.00" --request-id "cr-001"
```

| Flag | Required | Description |
|------|----------|-------------|
| `--consumer` | Yes | Consumer name or numeric consumer ID |
| `--amount` | Yes | Amount of credit to add |
| `--request-id` | Yes | Unique request ID for idempotency |

---

## JSON Output

All commands support `--format json` for machine-readable output, useful for scripting and automation:

```bash
llmpool-ctl --format json endpoint list
llmpool-ctl --format json consumer list
llmpool-ctl --format json fund show --consumer alice
```

## Name or ID Resolution

For `--endpoint` and `--consumer` flags, you can use either the name (string) or the numeric ID. The tool will automatically resolve names to IDs via the API.

```bash
# Both are equivalent
llmpool-ctl consumer show --consumer alice
llmpool-ctl consumer show --consumer 1
```

## Example Workflow

```bash
# Set up environment
export LLMPOOL_ADMIN_URL=http://localhost:19324
export LLMPOOL_ADMIN_TOKEN=$(llmpool admin create-jwt-token)

# Add an endpoint
llmpool-ctl endpoint add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1

# List detected models
llmpool-ctl model list

# Create a consumer and API key
llmpool-ctl consumer add alice
llmpool-ctl apikey add --consumer alice --label "development"

# Deposit funds
llmpool-ctl fund deposit --consumer alice --amount "100.00" --request-id "initial-deposit"

# Check balance
llmpool-ctl fund show --consumer alice
```
