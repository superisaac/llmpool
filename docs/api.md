# LLMPool API Reference

LLMPool provides three sets of APIs:

1. **OpenAI-Compatible API** — Standard OpenAI upstreams for AI model access
2. **Admin REST API** — RESTful management interface for administration
3. **Passthrough API** — Proxy requests to upstream upstreams

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

## OpenAI-Compatible Upstreams

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

The Admin API is a RESTful interface under `/api/v1/` and requires JWT authentication via the `x-admin-token` header. All list upstreams support pagination via `page` and `page_size` query parameters.

### Upstreams

```bash
# List all OpenAI upstreams (paginated)
curl http://localhost:19324/api/v1/upstreams \
  -H "x-admin-token: <jwt-token>"

# List upstreams with pagination
curl "http://localhost:19324/api/v1/upstreams?page=1&page_size=10" \
  -H "x-admin-token: <jwt-token>"

# Create a new upstream (auto-detects features and models)
curl -X POST http://localhost:19324/api/v1/upstreams \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "OpenAI",
    "api_key": "sk-xxx",
    "api_base": "https://api.openai.com/v1"
  }'
```

### Upstream Testing

```bash
# Test an upstream (detect features without saving)
curl -X POST http://localhost:19324/api/v1/upstream-tests \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "api_key": "sk-xxx",
    "api_base": "https://api.openai.com/v1"
  }'
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

# Get a model by ID
curl http://localhost:19324/api/v1/models/1 \
  -H "x-admin-token: <jwt-token>"

# Get a model by upstream name and model name
curl http://localhost:19324/api/v1/models/OpenAI/gpt-4o \
  -H "x-admin-token: <jwt-token>"

# Update a model's description
curl -X PUT http://localhost:19324/api/v1/models/1 \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"description": "GPT-4o model for general chat"}'
```

### Consumers

```bash
# List all consumers (paginated)
curl http://localhost:19324/api/v1/consumers \
  -H "x-admin-token: <jwt-token>"

# Create a consumer
curl -X POST http://localhost:19324/api/v1/consumers \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "alice"}'

# Create a consumer with initial credit
curl -X POST http://localhost:19324/api/v1/consumers \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "alice", "initial_credit": "100.00"}'

# Get a consumer by ID
curl http://localhost:19324/api/v1/consumers/1 \
  -H "x-admin-token: <jwt-token>"

# Get a consumer by name
curl http://localhost:19324/api/v1/consumers_by_name/alice \
  -H "x-admin-token: <jwt-token>"
```

### Funds

```bash
# Get a consumer's fund (balance information)
curl http://localhost:19324/api/v1/consumers/1/fund \
  -H "x-admin-token: <jwt-token>"
```

### API Keys

```bash
# List API keys for a consumer (paginated)
curl http://localhost:19324/api/v1/consumers/1/apikeys \
  -H "x-admin-token: <jwt-token>"

# Create an API key for a consumer
curl -X POST http://localhost:19324/api/v1/consumers/1/apikeys \
  -H "x-admin-token: <jwt-token>"
```

### Deposits, Withdrawals & Credits

The `unique_request_id` field is a client-provided idempotency key for each balance change operation. It ensures that the same request is not processed more than once.

```bash
# Create a deposit (adds to consumer's cash balance)
curl -X POST http://localhost:19324/api/v1/deposits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "consumer_id": 1,
    "unique_request_id": "deposit-20260326-001",
    "amount": "100.00"
  }'

# Create a withdrawal (deducts from consumer's cash balance)
curl -X POST http://localhost:19324/api/v1/withdrawals \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "consumer_id": 1,
    "unique_request_id": "withdraw-20260326-001",
    "amount": "50.00"
  }'

# Create a credit (adds to consumer's credit balance)
curl -X POST http://localhost:19324/api/v1/credits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "consumer_id": 1,
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

The session events list upstream uses cursor-based pagination. The response includes:
- `data`: Array of session event objects
- `next_id`: The ID of the last event in the current page (use as `start` for the next request)
- `has_more`: Whether there are more events after this page

### Admin REST API Reference

| Method | Upstream | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/upstreams` | List all OpenAI upstreams (paginated) |
| `POST` | `/api/v1/upstreams` | Create a new upstream (auto-detects features) |
| `POST` | `/api/v1/upstream-tests` | Test an upstream without saving |
| `POST` | `/api/v1/models-tests` | Test model features and update the LLMModel table |
| `GET` | `/api/v1/models` | List models (filterable, paginated) |
| `GET` | `/api/v1/models/:model_id` | Get a model by ID |
| `PUT` | `/api/v1/models/:model_id` | Update a model (description) |
| `GET` | `/api/v1/models/:upstream_name/*model_name` | Get a model by upstream name and model name |
| `GET` | `/api/v1/consumers` | List all consumers (paginated) |
| `POST` | `/api/v1/consumers` | Create a new consumer (with optional `initial_credit`) |
| `GET` | `/api/v1/consumers/:consumer_id` | Get a consumer by ID |
| `PUT` | `/api/v1/consumers/:consumer_id` | Update a consumer |
| `GET` | `/api/v1/consumers_by_name/:name` | Get a consumer by name |
| `GET` | `/api/v1/consumers/:consumer_id/fund` | Get a consumer's fund (cash, credit, debt) |
| `GET` | `/api/v1/consumers/:consumer_id/apikeys` | List API keys for a consumer (paginated) |
| `POST` | `/api/v1/consumers/:consumer_id/apikeys` | Create an API key for a consumer |
| `POST` | `/api/v1/deposits` | Create a deposit for a consumer |
| `POST` | `/api/v1/withdrawals` | Create a withdrawal for a consumer |
| `POST` | `/api/v1/credits` | Create a credit for a consumer |
| `GET` | `/api/v1/session-events` | List session events (cursor-based pagination) |
| `GET` | `/api/v1/session-events/:event_id` | Get a single session event by ID |

Most paginated upstreams accept the following query parameters:
- `page` (default: 1) — Page number (1-based)
- `page_size` (default: 20, max: 100) — Number of items per page

The `/api/v1/session-events` upstream uses cursor-based pagination:
- `start` (default: 0) — Event ID to start after (exclusive)
- `count` (default: 20, max: 100) — Number of items to return
- `session` (optional) — Filter by session_id

---

## Passthrough API

The Passthrough API proxies requests to upstream OpenAI-compatible upstreams. It requires JWT authentication via the `x-admin-token` header (same as the Admin REST API). The `x-admin-token` header is **not** forwarded to the upstream upstream.

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

| Method | Upstream | Description |
|--------|----------|-------------|
| `ANY` | `/passthrough/tag/:tag/*rest` | Proxy to a random upstream matching the tag |
| `ANY` | `/passthrough/:upstream_id/*rest` | Proxy to a specific upstream by ID |

> **Note:** The passthrough proxy automatically sets the `Authorization` header using the upstream's stored API key. Any `x-admin-token` header is stripped before forwarding to the upstream.
