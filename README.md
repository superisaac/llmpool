# LLMPool

LLMPool is an **OpenAI-compatible API gateway/proxy server** written in Rust. It aggregates multiple OpenAI-compatible backend endpoints (e.g., OpenAI, Azure OpenAI, self-hosted LLM services) behind a unified API, with built-in user management, API key authentication, usage tracking, and balance billing.

## Key Features

- **Multi-Endpoint Aggregation** — Register multiple OpenAI-compatible API endpoints with automatic detection of supported models and capabilities (Chat, Embedding, Image Generation, Speech)
- **Smart Routing & Retry** — Requests are randomly distributed across available endpoints; automatic retry on a different endpoint upon failure
- **OpenAI-Compatible API** — Standard `/v1/chat/completions`, `/v1/embeddings`, `/v1/images/generations`, `/v1/audio/speech`, and `/v1/models` endpoints
- **User & API Key Management** — Create users, generate API keys, and authenticate via Bearer Token
- **Usage Tracking & Billing** — Automatically records token usage per request, calculates costs based on model pricing, and manages cash balance, credit, and debt
- **Async Task Queue** — Redis + [Apalis](https://github.com/geofmureithi/apalis)-based async task processing for event logging and balance updates
- **Admin JSON-RPC API** — JWT-authenticated management interface for querying balances, creating users, and generating API keys
- **API Key Encryption** — OpenAI endpoint API keys are encrypted at rest in the database using AES algorithm; decryption happens transparently at runtime
- **OpenTelemetry Observability** — Built-in OpenTelemetry tracing support
- **Docker Support** — Includes Dockerfile and docker-compose.yml for one-command development environment setup

## Architecture Overview

```
┌─────────────┐     ┌──────────────────────────────────────────┐
│   Client    │────▶│              LLMPool Server              │
│(OpenAI SDK) │     │                                          │
└─────────────┘     │  /openai/v1/*   ──▶  OpenAI Proxy        │
                    │  /admin/rpc     ──▶  Admin JSON-RPC API  │
                    └──────────┬───────────────────┬───────────┘
                               │                   │
                    ┌──────────▼──┐     ┌──────────▼──────────┐
                    │ PostgreSQL  │     │  Redis (Task Queue) │
                    └─────────────┘     └──────────┬──────────┘
                                                   │
                                        ┌──────────▼──────────┐
                                        │ LLMPool Defer Worker│
                                        │ (Async Task Worker) │
                                        └─────────────────────┘
```

## Prerequisites

- **Rust** 1.90+ (Edition 2024)
- **PostgreSQL** 16+
- **Redis** 7+

## Installation

### Build from Source

```bash

# Build in release mode
cargo build --release

# The compiled binary is located at:
# target/release/llmpool
```

### Using Docker

```bash
cd docker

# Start infrastructure (PostgreSQL + Redis)
docker compose up -d postgres redis

# Run database migrations
docker compose run --rm llmpool-migrate

# Start application services
docker compose up llmpool llmpool-defer
```

## Configuration

Copy the example configuration file and modify it for your environment:

```bash
cp llmpool.toml.example llmpool.toml
```

Example `llmpool.toml`:

```toml
[database]
# PostgreSQL connection URL
url = "postgres://user:password@localhost/llmpool"

[admin]
# JWT secret for authenticating admin API requests
jwt_secret = "your-jwt-secret-here"

[redis]
# Redis connection URL (can also be set via REDIS_URL env var, which takes priority)
url = "redis://127.0.0.1:6379"

[security]
# Hex-encoded 256-bit key for AES-256-GCM encryption of API keys at rest.
# Generate with: openssl rand -hex 32
# Leave empty to store API keys in plaintext (not recommended for production).
encryption_key = "your-64-char-hex-key-here"
```

Config file resolution priority:
1. `--config <path>` CLI argument
2. `LLMPOOL_CONFIG` environment variable
3. `./llmpool.toml` in the current directory

## Common Commands

### Database Migration

```bash
llmpool migrate
```

### Start the Server

```bash
# Listen on default address 127.0.0.1:19324
llmpool serve

# Specify a custom bind address
llmpool serve --bind 0.0.0.0:19324
```

### Start the Async Task Worker

```bash
# Default concurrency of 4
llmpool worker

# Custom concurrency
llmpool worker --concurrency 8
```

### Manage OpenAI Endpoints

```bash
# Detect supported models and capabilities of an endpoint
llmpool openai detect --api-key sk-xxx --api-base https://api.openai.com/v1

# Add an endpoint (detect and save to database)
llmpool openai add --name "OpenAI" --api-key sk-xxx --api-base https://api.openai.com/v1
```

### Admin Operations

```bash
# Interactively create a user
llmpool admin create-user

# Create an API key for a user
llmpool admin create-api-key <username>

# Generate a non-expiring admin JWT token
llmpool admin create-jwt-token

# Generate a token that expires in 1 hour
llmpool admin create-jwt-token --expire 3600

# Specify a custom subject
llmpool admin create-jwt-token --subject admin --expire 86400
```

## API Usage

### OpenAI-Compatible Endpoints

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

Using the Python OpenAI SDK:

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

### Admin JSON-RPC API

The Admin API uses the JSON-RPC protocol and requires JWT Bearer Token authentication:

```bash
# Get user balance
curl http://localhost:19324/admin/rpc \
  -H "Authorization: Bearer <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getBalance",
    "params": {"user_id": 1},
    "id": 1
  }'

# Create a user
curl http://localhost:19324/admin/rpc \
  -H "Authorization: Bearer <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "createUser",
    "params": {"username": "alice"},
    "id": 1
  }'

# Create an API key
curl http://localhost:19324/admin/rpc \
  -H "Authorization: Bearer <jwt-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "createApiKey",
    "params": {"user_id": 1},
    "id": 1
  }'
```

## Quick Start

Here is a complete workflow from scratch:

```bash
# 1. Build the project
cargo build --release

# 2. Prepare the configuration file
cp llmpool.toml.example llmpool.toml
# Edit llmpool.toml with your database and Redis connection details

# 3. Run database migrations
./target/release/llmpool migrate

# 4. Add an OpenAI-compatible endpoint
./target/release/llmpool openai add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1

# 5. Create a user
./target/release/llmpool admin create-user

# 6. Create an API key for the user
./target/release/llmpool admin create-api-key <username>

# 7. Generate an admin JWT token
./target/release/llmpool admin create-jwt-token

# 8. Start the async task worker (in a separate terminal)
./target/release/llmpool worker

# 9. Start the server
./target/release/llmpool serve --bind 0.0.0.0:19324
```

## Environment Variables

| Variable | Description | Priority |
|----------|-------------|----------|
| `LLMPOOL_CONFIG` | Path to the configuration file | Higher than default path, lower than `--config` flag |
| `REDIS_URL` | Redis connection URL | Higher than `[redis] url` in config file |
| `DATABASE_URL` | Database connection URL (used in Docker) | — |

## License

MIT
