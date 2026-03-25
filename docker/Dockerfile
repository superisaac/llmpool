# ============================================================
# Multi-stage Dockerfile for llmpool
# ============================================================

# Stage 1: Build
FROM rust:1.90-bookworm AS builder

WORKDIR /app

# Install system dependencies required by sqlx (libpq for PostgreSQL)
RUN apt-get update && apt-get install -y libpq-dev && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests first for better layer caching
COPY Cargo.toml Cargo.lock* ./
COPY crates/llmpool/Cargo.toml crates/llmpool/Cargo.toml

# Create dummy source files to pre-build dependencies
RUN mkdir -p crates/llmpool/src && echo "fn main() {}" > crates/llmpool/src/main.rs

RUN cargo build --release --bin llmpool 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/

# Touch main.rs to force rebuild of our code (not dependencies)
RUN touch crates/llmpool/src/main.rs

# Build the release binary
RUN cargo build --release --bin llmpool

# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the compiled binary
COPY --from=builder /app/target/release/llmpool /usr/local/bin/llmpool

# Copy migrations (used by sqlx::migrate! at runtime)
COPY crates/llmpool/migrations/ /app/migrations/

EXPOSE 19324

ENTRYPOINT ["llmpool"]
CMD ["serve", "--bind", "0.0.0.0:19324"]
