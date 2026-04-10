# LLMPool API Reference

LLMPool provides three sets of APIs:

1. **OpenAI-Compatible API** — Standard OpenAI endpoints for AI model access
2. **Admin REST API** — RESTful management interface for administration
3. **Passthrough API** — Proxy requests to upstream backends

Both the Admin REST API and Passthrough API use the `x-admin-token` header for JWT authentication.

---

## Authentication

### OpenAI-Compatible API

Uses standard `Authorization: Bearer <api-key>` header with user API keys.

### Admin REST API & Passthrough API

Both use the `x-admin-token` HTTP header containing a JWT token signed with the admin JWT secret configured in `llmpool.toml`:

```
x-admin-token: <jwt-token>
```

The JWT token is validated against the `[admin] jwt_secret` in the configuration file.

---

## OpenAI-Compatible Endpoints

LLMPool exposes standard OpenAI-compatible APIs. You can use any OpenAI SDK or compatible client directly:

```bash
# List available models
curl http://localhost:19324/openai/v1/models \
  -H "Authorization: Bearer <your-api-key>"

# Chat Completions
curl http://localhost:19324/openai/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Chat Completions (Streaming)
curl http://localhost:19324/openai/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'

# Embeddings
curl http://localhost:19324/openai/v1/embeddings \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "text-embedding-3-small",
    "input": "Hello world"
  }'

# Image Generation
curl http://localhost:19324/openai/v1/images/generations \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "dall-e-3",
    "prompt": "A cute cat"
  }'
```

### Using the Python OpenAI SDK

```python
from openai import OpenAI

client = OpenAI(
    api_key="your-api-key",
    base_url="http://localhost:19324/openai/v1"
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)
```

---

## Admin REST API

The Admin API is a RESTful interface under `/api/v1/` and requires JWT authentication via the `x-admin-token` header. All list endpoints support pagination via `page` and `page_size` query parameters.

### Upstreams

```bash
# List all upstreams (paginated)
curl http://localhost:19324/api/v1/upstreams \
  -H "x-admin-token: <jwt-token>"

# List upstreams with pagination
curl "http://localhost:19324/api/v1/upstreams?page=1&page_size=10" \
  -H "x-admin-token: <jwt-token>"

# Create a new upstream (auto-detects models)
curl -X POST http://localhost:19324/api/v1/upstreams \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "OpenAI",
    "api_key": "sk-xxx",
    "api_base": "https://api.openai.com/v1"
  }'

# Get upstream by ID
curl http://localhost:19324/api/v1/upstreams/1 \
  -H "x-admin-token: <jwt-token>"

# Get upstream by name
curl http://localhost:19324/api/v1/upstream_by_name/OpenAI \
  -H "x-admin-token: <jwt-token>"

# Update an upstream
curl -X PUT http://localhost:19324/api/v1/upstreams/1 \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"description": "Main OpenAI upstream", "status": "online"}'
```

### Upstream Tag Management

```bash
# List tags for an upstream
curl http://localhost:19324/api/v1/upstreams/1/tags \
  -H "x-admin-token: <jwt-token>"

# Add a tag to an upstream
curl -X POST http://localhost:19324/api/v1/upstreams/1/tags \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"tag": "production"}'

# Remove a tag from an upstream
curl -X DELETE http://localhost:19324/api/v1/upstreams/1/tags/production \
  -H "x-admin-token: <jwt-token>"
```

### Model Testing

```bash
# Test features of specific models by their database IDs and update the LLMModel table
curl -X POST http://localhost:19324/api/v1/models-tests \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"model_ids": [1, 2, 3]}'
```

The response is an array of per-model results. Each entry contains either the updated model
object (with refreshed feature flags) or an error message if the test failed for that model.

### Models

```bash
# List all models (paginated)
curl http://localhost:19324/api/v1/models \
  -H "x-admin-token: <jwt-token>"

# Filter models by upstream ID, upstream name, or model name
curl "http://localhost:19324/api/v1/models?upstream_id=1" \
  -H "x-admin-token: <jwt-token>"

curl "http://localhost:19324/api/v1/models?upstream_name=OpenAI&name=gpt-4o" \
  -H "x-admin-token: <jwt-token>"

# Filter models by active status
curl "http://localhost:19324/api/v1/models?is_active=true" \
  -H "x-admin-token: <jwt-token>"

# Get a model by ID
curl http://localhost:19324/api/v1/models/1 \
  -H "x-admin-token: <jwt-token>"

# Get a model by upstream name and model name
curl http://localhost:19324/api/v1/models/path/OpenAI/gpt-4o \
  -H "x-admin-token: <jwt-token>"

# Update a model's description and pricing
curl -X PUT http://localhost:19324/api/v1/models/1 \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"description": "GPT-4o model for general chat"}'
```

### Accounts

```bash
# List all accounts (paginated)
curl http://localhost:19324/api/v1/accounts \
  -H "x-admin-token: <jwt-token>"

# Create an account
curl -X POST http://localhost:19324/api/v1/accounts \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "alice"}'

# Get an account by ID
curl http://localhost:19324/api/v1/accounts/1 \
  -H "x-admin-token: <jwt-token>"

# Get an account by name
curl http://localhost:19324/api/v1/accounts_by_name/alice \
  -H "x-admin-token: <jwt-token>"

# Update an account
curl -X PUT http://localhost:19324/api/v1/accounts/1 \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"is_active": false}'
```

### Wallets

Each account has a wallet that tracks its balance. The balance is a single decimal value that can be negative (indicating debt). Deposits and credits add to the balance; withdrawals and token spending deduct from it.

```bash
# Get an account's wallet (balance information)
curl http://localhost:19324/api/v1/accounts/1/wallet \
  -H "x-admin-token: <jwt-token>"
```

### API Keys

```bash
# List API keys for an account (paginated)
curl http://localhost:19324/api/v1/accounts/1/apikeys \
  -H "x-admin-token: <jwt-token>"

# Create an API key for an account
curl -X POST http://localhost:19324/api/v1/accounts/1/apikeys \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"label": "dev key"}'

# Get API key info by key string
curl http://localhost:19324/api/v1/apikeys/lpx-xxx \
  -H "x-admin-token: <jwt-token>"

# Deactivate an API key
curl -X DELETE http://localhost:19324/api/v1/apikeys/lpx-xxx \
  -H "x-admin-token: <jwt-token>"
```

### Deposits, Withdrawals & Credits

The `unique_request_id` field is a client-provided idempotency key for each balance change operation. It ensures that the same request is not processed more than once.

```bash
# Create a deposit (adds to account's wallet balance)
curl -X POST http://localhost:19324/api/v1/deposits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": 1,
    "unique_request_id": "deposit-20260326-001",
    "amount": "100.00"
  }'

# Create a withdrawal (deducts from account's wallet balance)
curl -X POST http://localhost:19324/api/v1/withdrawals \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": 1,
    "unique_request_id": "withdraw-20260326-001",
    "amount": "50.00"
  }'

# Create a credit (adds to account's wallet balance, same as deposit)
curl -X POST http://localhost:19324/api/v1/credits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": 1,
    "unique_request_id": "credit-20260326-001",
    "amount": "200.00"
  }'
```

### Session Events

```bash
# Get a single session event by ID
curl "http://localhost:19324/api/v1/session-events/1" \
  -H "x-admin-token: <jwt-token>"

# List session events (cursor-based pagination)
curl "http://localhost:19324/api/v1/session-events" \
  -H "x-admin-token: <jwt-token>"

# List with cursor parameters
curl "http://localhost:19324/api/v1/session-events?start=0&count=50" \
  -H "x-admin-token: <jwt-token>"

# Filter by session ID
curl "http://localhost:19324/api/v1/session-events?session=sess-abc123" \
  -H "x-admin-token: <jwt-token>"

# Paginate using next_id from previous response
curl "http://localhost:19324/api/v1/session-events?start=42&count=20" \
  -H "x-admin-token: <jwt-token>"
```

The session events list endpoint uses cursor-based pagination. The response includes:
- `data`: Array of session event objects
- `next_id`: The ID of the last event in the current page (use as `start` for the next request)
- `has_more`: Whether there are more events after this page

### Admin REST API Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/upstreams` | List all upstreams (paginated) |
| `POST` | `/api/v1/upstreams` | Create a new upstream (auto-detects models) |
| `GET` | `/api/v1/upstreams/:upstream_id` | Get an upstream by ID |
| `PUT` | `/api/v1/upstreams/:upstream_id` | Update an upstream |
| `GET` | `/api/v1/upstream_by_name/:name` | Get an upstream by name |
| `GET` | `/api/v1/upstreams/:upstream_id/tags` | List tags for an upstream |
| `POST` | `/api/v1/upstreams/:upstream_id/tags` | Add a tag to an upstream |
| `DELETE` | `/api/v1/upstreams/:upstream_id/tags/:tag` | Remove a tag from an upstream |
| `POST` | `/api/v1/models-tests` | Test model features and update the LLMModel table |
| `GET` | `/api/v1/models` | List models (filterable, paginated) |
| `GET` | `/api/v1/models/:model_id` | Get a model by ID |
| `PUT` | `/api/v1/models/:model_id` | Update a model (description, pricing, active status) |
| `GET` | `/api/v1/models/path/:upstream_name/*model_name` | Get a model by upstream name and model name |
| `GET` | `/api/v1/accounts` | List all accounts (paginated) |
| `POST` | `/api/v1/accounts` | Create a new account |
| `GET` | `/api/v1/accounts/:account_id` | Get an account by ID |
| `PUT` | `/api/v1/accounts/:account_id` | Update an account |
| `GET` | `/api/v1/accounts_by_name/:name` | Get an account by name |
| `GET` | `/api/v1/accounts/:account_id/wallet` | Get an account's wallet (balance) |
| `GET` | `/api/v1/accounts/:account_id/apikeys` | List API keys for an account (paginated) |
| `POST` | `/api/v1/accounts/:account_id/apikeys` | Create an API key for an account |
| `GET` | `/api/v1/apikeys/:apikey` | Get API key info by key string |
| `DELETE` | `/api/v1/apikeys/:apikey` | Deactivate an API key |
| `POST` | `/api/v1/deposits` | Create a deposit for an account |
| `POST` | `/api/v1/withdrawals` | Create a withdrawal for an account |
| `POST` | `/api/v1/credits` | Create a credit for an account |
| `GET` | `/api/v1/session-events` | List session events (cursor-based pagination) |
| `GET` | `/api/v1/session-events/:event_id` | Get a single session event by ID |
| `GET` | `/api/v1/subscription-plans` | List subscription plans (paginated) |
| `POST` | `/api/v1/subscription-plans` | Create a subscription plan |
| `GET` | `/api/v1/subscription-plans/:plan_id` | Get a subscription plan by ID |
| `PUT` | `/api/v1/subscription-plans/:plan_id` | Update a subscription plan |
| `DELETE` | `/api/v1/subscription-plans/:plan_id` | Cancel a subscription plan |
| `GET` | `/api/v1/subscriptions` | List subscriptions (filterable, paginated) |
| `POST` | `/api/v1/subscriptions` | Create a subscription |
| `GET` | `/api/v1/subscriptions/:subscription_id` | Get a subscription by ID |
| `PUT` | `/api/v1/subscriptions/:subscription_id` | Update a subscription status |
| `DELETE` | `/api/v1/subscriptions/:subscription_id` | Cancel a subscription |

Most paginated endpoints accept the following query parameters:
- `page` (default: 1) — Page number (1-based)
- `page_size` (default: 20, max: 100) — Number of items per page

The `/api/v1/session-events` endpoint uses cursor-based pagination:
- `start` (default: 0) — Event ID to start after (exclusive)
- `count` (default: 20, max: 100) — Number of items to return
- `session` (optional) — Filter by session_id

---

## Passthrough API

The Passthrough API proxies requests to upstream OpenAI-compatible backends. It requires JWT authentication via the `x-admin-token` header (same as the Admin REST API). The `x-admin-token` header is **not** forwarded to the upstream backend.

### By Tag

Proxies the request to a randomly selected upstream matching the given tag:

```bash
# Proxy a chat completion request to an upstream tagged "openai"
curl -X POST http://localhost:19324/passthrough/tag/openai/v1/chat/completions \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### By Upstream ID

Proxies the request to a specific upstream by its ID:

```bash
# Proxy a chat completion request to upstream with ID 1
curl -X POST http://localhost:19324/passthrough/1/v1/chat/completions \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Passthrough API Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `ANY` | `/passthrough/tag/:tag/*rest` | Proxy to a random upstream matching the tag |
| `ANY` | `/passthrough/:upstream_id/*rest` | Proxy to a specific upstream by ID |

> **Note:** The passthrough proxy automatically sets the `Authorization` header using the upstream's stored API key. Any `x-admin-token` header is stripped before forwarding to the upstream.
