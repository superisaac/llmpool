# LLMPool

LLMPool is an **OpenAI-compatible API gateway/proxy server** written in Rust. It aggregates multiple OpenAI-compatible backend upstreams (e.g., OpenAI, Azure OpenAI, self-hosted LLM services) behind a unified API, with built-in user management, API key authentication, usage tracking, and balance billing.

## Key Features

- **Multi-Upstream Aggregation** — Register multiple OpenAI-compatible API upstreams with automatic detection of supported models and capabilities (Chat, Embedding, Image Generation, Speech)
- **Smart Routing & Retry** — Requests are randomly distributed across available upstreams; automatic retry on a different upstream upon failure
- **OpenAI-Compatible API** — Standard `/v1/chat/completions`, `/v1/embeddings`, `/v1/images/generations`, `/v1/audio/speech`, and `/v1/models` endpoints
- **Account & API Key Management** — Create accounts, generate API keys, and authenticate via Bearer Token
- **Wallet & Billing** — Each account has a wallet with a single balance field. Token spending, deposits, withdrawals, and credits are tracked as balance change records. Balance can go negative (debt).
- **Subscription Plans** — Define token/money limits per account via subscription plans; spending is deducted from the active subscription before touching the wallet balance
- **Async Task Queue** — Redis + [Apalis](https://github.com/geofmureithi/apalis)-based async task processing for event logging and balance updates
- **Admin REST API** — JWT-authenticated RESTful management interface for managing upstreams, models, accounts, API keys, wallets, and subscriptions with paginated responses
- **API Key Encryption** — OpenAI upstream API keys are encrypted at rest in the database using AES algorithm; decryption happens transparently at runtime
- **OpenTelemetry Observability** — Built-in OpenTelemetry tracing support
- **Docker Support** — Includes Dockerfile and docker-compose.yml for one-command development environment setup

## Architecture Overview

```
┌─────────────┐     ┌──────────────────────────────────────────┐
│   Client    │────▶│              LLMPool Server              │
│(OpenAI SDK) │     │                                          │
└─────────────┘     │  /openai/v1/*   ──▶  OpenAI Proxy        │
                    │  /api/v1/*      ──▶  Admin REST API      │
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

### Manage OpenAI Upstreams

```bash
# Detect supported models and capabilities of an upstream
llmpool openai detect --api-key sk-xxx --api-base https://api.openai.com/v1

# Add an upstream (detect and save to database)
llmpool openai add --name "OpenAI" --api-key sk-xxx --api-base https://api.openai.com/v1
```

### Admin Operations

```bash
# Interactively create a user
llmpool admin create-user

# Create an API key for a user
llmpool admin create-api-key <name>

# Generate a non-expiring admin JWT token
llmpool admin create-jwt-token

# Generate a token that expires in 1 hour
llmpool admin create-jwt-token --expire 3600

# Specify a custom subject
llmpool admin create-jwt-token --subject admin --expire 86400
```

## API Usage

For detailed API documentation with examples, see the **[API Reference](docs/api.md)**.

For the `llmpool-ctl` CLI management tool documentation, see the **[llmpool-ctl Reference](docs/controls.md)**.

LLMPool provides two sets of APIs:

- **OpenAI-Compatible API** (`/openai/v1/*`) — Standard endpoints for Chat Completions, Embeddings, Image Generation, Speech, and Models. Compatible with any OpenAI SDK.
- **Admin REST API** (`/api/v1/*`) — JWT-authenticated RESTful interface for managing upstreams, models, accounts, API keys, wallets, and billing (deposits, withdrawals, credits).

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

# 4. Add an OpenAI-compatible upstream
./target/release/llmpool openai add \
  --name "OpenAI" \
  --api-key sk-xxx \
  --api-base https://api.openai.com/v1

# 5. Create a user
./target/release/llmpool admin create-user

# 6. Create an API key for the user
./target/release/llmpool admin create-api-key <name>

# 7. Generate an admin JWT token
./target/release/llmpool admin create-jwt-token

# 8. Start the async task worker (in a separate terminal)
./target/release/llmpool worker

# 9. Start the server
./target/release/llmpool serve --bind 0.0.0.0:19324
```

## RUN tests
```
DATABASE_URL="postgres://localhost/llmpool_test" cargo test --test db_tests -- --list
```

## Environment Variables

| Variable | Description | Priority |
|----------|-------------|----------|
| `LLMPOOL_CONFIG` | Path to the configuration file | Higher than default path, lower than `--config` flag |
| `REDIS_URL` | Redis connection URL | Higher than `[redis] url` in config file |
| `DATABASE_URL` | Database connection URL (used in Docker) | — |

## License

MIT
