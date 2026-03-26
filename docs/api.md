# LLMPool API Reference

LLMPool provides three sets of APIs:

1. **OpenAI-Compatible API** — Standard OpenAI endpoints for AI model access
2. **Admin REST API** — RESTful management interface for administration
3. **Passthrough API** — Proxy requests to upstream endpoints

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

### Endpoints

```bash
# List all OpenAI endpoints (paginated)
curl http://localhost:19324/api/v1/endpoints \
  -H "x-admin-token: <jwt-token>"

# List endpoints with pagination
curl "http://localhost:19324/api/v1/endpoints?page=1&page_size=10" \
  -H "x-admin-token: <jwt-token>"

# Create a new endpoint (auto-detects features and models)
curl -X POST http://localhost:19324/api/v1/endpoints \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "OpenAI",
    "api_key": "sk-xxx",
    "api_base": "https://api.openai.com/v1"
  }'
```

### Endpoint Testing

```bash
# Test an endpoint (detect features without saving)
curl -X POST http://localhost:19324/api/v1/endpoint-tests \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "api_key": "sk-xxx",
    "api_base": "https://api.openai.com/v1"
  }'
```

### Models

```bash
# List all models (paginated)
curl http://localhost:19324/api/v1/models \
  -H "x-admin-token: <jwt-token>"

# Filter models by endpoint ID, endpoint name, or model name
curl "http://localhost:19324/api/v1/models?endpoint_id=1" \
  -H "x-admin-token: <jwt-token>"

curl "http://localhost:19324/api/v1/models?endpoint_name=OpenAI&name=gpt-4o" \
  -H "x-admin-token: <jwt-token>"
```

### Users

```bash
# List all users (paginated)
curl http://localhost:19324/api/v1/users \
  -H "x-admin-token: <jwt-token>"

# Create a user
curl -X POST http://localhost:19324/api/v1/users \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"username": "alice"}'

# Create a user with initial credit
curl -X POST http://localhost:19324/api/v1/users \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{"username": "alice", "initial_credit": "100.00"}'

# Get a user by ID
curl http://localhost:19324/api/v1/users/1 \
  -H "x-admin-token: <jwt-token>"

# Get a user by username
curl http://localhost:19324/api/v1/users_by_name/alice \
  -H "x-admin-token: <jwt-token>"
```

### Funds

```bash
# Get a user's fund (balance information)
curl http://localhost:19324/api/v1/users/1/fund \
  -H "x-admin-token: <jwt-token>"
```

### API Keys

```bash
# List API keys for a user (paginated)
curl http://localhost:19324/api/v1/users/1/apikeys \
  -H "x-admin-token: <jwt-token>"

# Create an API key for a user
curl -X POST http://localhost:19324/api/v1/users/1/apikeys \
  -H "x-admin-token: <jwt-token>"
```

### Deposits, Withdrawals & Credits

The `unique_request_id` field is a client-provided idempotency key for each balance change operation. It ensures that the same request is not processed more than once.

```bash
# Create a deposit (adds to user's cash balance)
curl -X POST http://localhost:19324/api/v1/deposits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": 1,
    "unique_request_id": "deposit-20260326-001",
    "amount": "100.00"
  }'

# Create a withdrawal (deducts from user's cash balance)
curl -X POST http://localhost:19324/api/v1/withdrawals \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": 1,
    "unique_request_id": "withdraw-20260326-001",
    "amount": "50.00"
  }'

# Create a credit (adds to user's credit balance)
curl -X POST http://localhost:19324/api/v1/credits \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": 1,
    "unique_request_id": "credit-20260326-001",
    "amount": "200.00"
  }'
```

### Admin REST API Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/endpoints` | List all OpenAI endpoints (paginated) |
| `POST` | `/api/v1/endpoints` | Create a new endpoint (auto-detects features) |
| `POST` | `/api/v1/endpoint-tests` | Test an endpoint without saving |
| `GET` | `/api/v1/models` | List models (filterable, paginated) |
| `GET` | `/api/v1/users` | List all users (paginated) |
| `POST` | `/api/v1/users` | Create a new user (with optional `initial_credit`) |
| `GET` | `/api/v1/users/:user_id` | Get a user by ID |
| `GET` | `/api/v1/users_by_name/:username` | Get a user by username |
| `GET` | `/api/v1/users/:user_id/fund` | Get a user's fund (cash, credit, debt) |
| `GET` | `/api/v1/users/:user_id/apikeys` | List API keys for a user (paginated) |
| `POST` | `/api/v1/users/:user_id/apikeys` | Create an API key for a user |
| `POST` | `/api/v1/deposits` | Create a deposit for a user |
| `POST` | `/api/v1/withdrawals` | Create a withdrawal for a user |
| `POST` | `/api/v1/credits` | Create a credit for a user |

All paginated endpoints accept the following query parameters:
- `page` (default: 1) — Page number (1-based)
- `page_size` (default: 20, max: 100) — Number of items per page

---

## Passthrough API

The Passthrough API proxies requests to upstream OpenAI-compatible endpoints. It requires JWT authentication via the `x-admin-token` header (same as the Admin REST API). The `x-admin-token` header is **not** forwarded to the upstream endpoint.

### By Tag

Proxies the request to a randomly selected endpoint matching the given tag:

```bash
# Proxy a chat completion request to an endpoint tagged "openai"
curl -X POST http://localhost:19324/passthrough/tag/openai/v1/chat/completions \
  -H "x-admin-token: <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### By Endpoint ID

Proxies the request to a specific endpoint by its ID:

```bash
# Proxy a chat completion request to endpoint with ID 1
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
| `ANY` | `/passthrough/tag/:tag/*rest` | Proxy to a random endpoint matching the tag |
| `ANY` | `/passthrough/:endpoint_id/*rest` | Proxy to a specific endpoint by ID |

> **Note:** The passthrough proxy automatically sets the `Authorization` header using the endpoint's stored API key. Any `x-admin-token` header is stripped before forwarding to the upstream.
