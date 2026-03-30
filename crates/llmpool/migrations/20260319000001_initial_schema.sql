-- Create llm_endpoints table
CREATE TABLE IF NOT EXISTS llm_endpoints (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    api_base VARCHAR NOT NULL,
    api_key VARCHAR NOT NULL DEFAULT '',
    has_responses_api BOOLEAN NOT NULL DEFAULT FALSE,
    tags TEXT[] NOT NULL DEFAULT '{}',
    proxies TEXT[] NOT NULL DEFAULT '{}',
    status VARCHAR NOT NULL DEFAULT 'online',
    description VARCHAR NOT NULL DEFAULT '',
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_llm_endpoints_api_base ON llm_endpoints (api_base);
CREATE INDEX IF NOT EXISTS idx_llm_endpoints_tags ON llm_endpoints USING GIN (tags);

-- Create llm_models table
CREATE TABLE IF NOT EXISTS llm_models (
    id SERIAL PRIMARY KEY,
    endpoint_id INTEGER NOT NULL REFERENCES llm_endpoints(id) ON DELETE CASCADE,
    model_id VARCHAR NOT NULL,
    has_image_generation BOOLEAN NOT NULL DEFAULT FALSE,
    has_speech BOOLEAN NOT NULL DEFAULT FALSE,
    has_chat_completion BOOLEAN NOT NULL DEFAULT FALSE,
    has_embedding BOOLEAN NOT NULL DEFAULT FALSE,
    input_token_price NUMERIC NOT NULL DEFAULT 0.000001,
    output_token_price NUMERIC NOT NULL DEFAULT 0.000001,
    description VARCHAR NOT NULL DEFAULT '',
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_models_endpoint_model ON llm_models (endpoint_id, model_id);
CREATE INDEX IF NOT EXISTS idx_llm_models_endpoint_id ON llm_models (endpoint_id);

-- Create accounts table
CREATE TABLE IF NOT EXISTS accounts (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_name ON accounts (name);

-- Create llm_api_keys table (formerly openai_api_keys)
CREATE TABLE IF NOT EXISTS llm_api_keys (
    id SERIAL PRIMARY KEY,
    consumer_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    apikey VARCHAR NOT NULL,
    label VARCHAR NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_openai_api_keys_apikey ON llm_api_keys (apikey);
CREATE INDEX IF NOT EXISTS idx_openai_api_keys_consumer_id ON llm_api_keys (consumer_id);

-- Create session_events table (unlogged for performance)
CREATE UNLOGGED TABLE IF NOT EXISTS session_events (
    id BIGSERIAL PRIMARY KEY,
    session_id VARCHAR NOT NULL,
    session_index INT NOT NULL DEFAULT 0,
    consumer_id INT NOT NULL,
    model_id INT NOT NULL,
    api_key_id INT NOT NULL DEFAULT 0,
    input_token_price NUMERIC NOT NULL DEFAULT 0,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_token_price NUMERIC NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    event_data JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_session_events_session_id_index ON session_events (session_id, session_index);
CREATE INDEX IF NOT EXISTS idx_session_events_session_id ON session_events (session_id);

-- Create funds table
CREATE TABLE IF NOT EXISTS funds (
    id SERIAL PRIMARY KEY,
    consumer_id INT NOT NULL REFERENCES accounts(id),
    cash DECIMAL NOT NULL DEFAULT 0,
    credit DECIMAL NOT NULL DEFAULT 0,
    debt DECIMAL NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_funds_consumer_id ON funds (consumer_id);

-- Create balance_changes table
CREATE TABLE IF NOT EXISTS balance_changes (
    id SERIAL PRIMARY KEY,
    consumer_id INT NOT NULL REFERENCES accounts(id),
    unique_request_id VARCHAR NOT NULL,
    content JSONB NOT NULL DEFAULT '{}',
    is_applied BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_balance_changes_consumer_id ON balance_changes (consumer_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_balance_changes_unique_request_id ON balance_changes (unique_request_id);
