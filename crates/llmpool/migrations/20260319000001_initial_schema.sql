-- Create openai_endpoints table
CREATE TABLE IF NOT EXISTS openai_endpoints (
    id SERIAL PRIMARY KEY,
    name VARCHAR NOT NULL,
    api_base VARCHAR NOT NULL,
    api_key VARCHAR NOT NULL DEFAULT '',
    has_responses_api BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_openai_endpoints_api_base ON openai_endpoints (api_base);

-- Create openai_models table
CREATE TABLE IF NOT EXISTS openai_models (
    id SERIAL PRIMARY KEY,
    endpoint_id INTEGER NOT NULL REFERENCES openai_endpoints(id) ON DELETE CASCADE,
    model_id VARCHAR NOT NULL,
    has_image_generation BOOLEAN NOT NULL DEFAULT FALSE,
    has_speech BOOLEAN NOT NULL DEFAULT FALSE,
    has_chat_completion BOOLEAN NOT NULL DEFAULT FALSE,
    has_embedding BOOLEAN NOT NULL DEFAULT FALSE,
    input_token_price NUMERIC NOT NULL DEFAULT 0.000001,
    output_token_price NUMERIC NOT NULL DEFAULT 0.000001,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_openai_models_endpoint_model ON openai_models (endpoint_id, model_id);
CREATE INDEX IF NOT EXISTS idx_openai_models_endpoint_id ON openai_models (endpoint_id);

-- Create users table
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username VARCHAR NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username ON users (username);

-- Create access_keys table
CREATE TABLE IF NOT EXISTS access_keys (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    apikey VARCHAR NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_access_keys_apikey ON access_keys (apikey);
CREATE INDEX IF NOT EXISTS idx_access_keys_user_id ON access_keys (user_id);

-- Create session_events table (unlogged for performance)
CREATE UNLOGGED TABLE IF NOT EXISTS session_events (
    id BIGSERIAL PRIMARY KEY,
    session_id VARCHAR NOT NULL,
    user_id INT NOT NULL,
    model_id INT NOT NULL,
    event_data JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_session_events_session_id ON session_events (session_id);

-- Create user_balances table
CREATE TABLE IF NOT EXISTS user_balances (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL REFERENCES users(id),
    cash DECIMAL NOT NULL DEFAULT 0,
    credit DECIMAL NOT NULL DEFAULT 0,
    debt DECIMAL NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_balances_user_id ON user_balances (user_id);

-- Create balance_changes table
CREATE TABLE IF NOT EXISTS balance_changes (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL REFERENCES users(id),
    content JSONB NOT NULL DEFAULT '{}',
    is_applied BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_balance_changes_user_id ON balance_changes (user_id);
