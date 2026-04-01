-- Create llm_upstreams table
CREATE TABLE IF NOT EXISTS llm_upstreams (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    api_base VARCHAR NOT NULL,
    encrypted_api_key VARCHAR NOT NULL DEFAULT '',
    ellipsed_api_key VARCHAR NOT NULL DEFAULT '',
    provider VARCHAR NOT NULL DEFAULT 'openai',
    has_responses_api BOOLEAN NOT NULL DEFAULT FALSE,
    tags TEXT[] NOT NULL DEFAULT '{}',
    proxies TEXT[] NOT NULL DEFAULT '{}',
    status VARCHAR NOT NULL DEFAULT 'online',
    description VARCHAR NOT NULL DEFAULT '',
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_llm_upstreams_api_base ON llm_upstreams (api_base);
CREATE INDEX IF NOT EXISTS idx_llm_upstreams_tags ON llm_upstreams USING GIN (tags);

-- Create llm_models table
CREATE TABLE IF NOT EXISTS llm_models (
    id SERIAL PRIMARY KEY,
    upstream_id INTEGER NOT NULL REFERENCES llm_upstreams(id) ON DELETE CASCADE,
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_models_upstream_model ON llm_models (upstream_id, model_id);
CREATE INDEX IF NOT EXISTS idx_llm_models_upstream_id ON llm_models (upstream_id);

-- Create accounts table
CREATE TABLE IF NOT EXISTS accounts (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_name ON accounts (name);

-- Create api_credentials table
CREATE TABLE IF NOT EXISTS api_credentials (
    id SERIAL PRIMARY KEY,
    account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    apikey VARCHAR NOT NULL,
    label VARCHAR NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_credentials_apikey ON api_credential (apikey);
CREATE INDEX IF NOT EXISTS idx_api_credentials_account_id ON api_credential (account_id);

-- Create session_events table (unlogged for performance)
CREATE UNLOGGED TABLE IF NOT EXISTS session_events (
    id BIGSERIAL PRIMARY KEY,
    session_id VARCHAR NOT NULL,
    session_index INT NOT NULL DEFAULT 0,
    account_id INT NOT NULL,
    model_id INT NOT NULL,
    api_credential_id INT NOT NULL DEFAULT 0,
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
    account_id INT NOT NULL REFERENCES accounts(id),
    cash DECIMAL NOT NULL DEFAULT 0,
    debt DECIMAL NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_funds_account_id ON funds (account_id);

-- Create file_metas table
CREATE TABLE IF NOT EXISTS file_metas (
    id BIGSERIAL PRIMARY KEY,
    file_id VARCHAR NOT NULL,
    original_file_id VARCHAR NOT NULL DEFAULT '',
    purpose VARCHAR NOT NULL DEFAULT '',
    upstream_id INTEGER NOT NULL DEFAULT 0,
    deleted BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_file_metas_file_id ON file_metas (file_id);
CREATE INDEX IF NOT EXISTS idx_file_metas_original_file_id ON file_metas (original_file_id);

-- Create batch_metas table
CREATE TABLE IF NOT EXISTS batch_metas (
    id BIGSERIAL PRIMARY KEY,
    batch_id VARCHAR NOT NULL,
    original_batch_id VARCHAR NOT NULL DEFAULT '',
    upstream_id INTEGER NOT NULL DEFAULT 0,
    status VARCHAR NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_batch_metas_batch_id ON batch_metas (batch_id);
CREATE INDEX IF NOT EXISTS idx_batch_metas_original_batch_id ON batch_metas (original_batch_id);

-- Create balance_changes table
CREATE TABLE IF NOT EXISTS balance_changes (
    id SERIAL PRIMARY KEY,
    account_id INT NOT NULL REFERENCES accounts(id),
    unique_request_id VARCHAR NOT NULL,
    content JSONB NOT NULL DEFAULT '{}',
    is_applied BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_balance_changes_account_id ON balance_changes (account_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_balance_changes_unique_request_id ON balance_changes (unique_request_id);
