use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Global application configuration, loaded once from a TOML file.
static CONFIG: OnceLock<AppConfig> = OnceLock::new();

/// Top-level application configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    #[serde(default)]
    pub admin: AdminConfig,
    #[serde(default)]
    pub redis: RedisConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

/// Database configuration section.
#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL connection URL, e.g. "postgres://user:pass@localhost/dbname"
    pub url: String,
}

/// Admin API configuration section.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct AdminConfig {
    /// JWT secret used to authenticate admin API requests
    #[serde(default)]
    pub jwt_secret: String,
}

/// Redis configuration section.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct RedisConfig {
    /// Redis connection URL, e.g. "redis://127.0.0.1:6379"
    #[serde(default)]
    pub url: String,
}

/// Security configuration section.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct SecurityConfig {
    /// Hex-encoded 256-bit (32-byte) key used for AES-256-GCM encryption of sensitive fields
    /// (e.g., OpenAI upstream API keys). Generate with: `openssl rand -hex 32`
    #[serde(default)]
    pub encryption_key: String,
}

/// Resolve the config file path.
///
/// Priority:
/// 1. `--config <path>` CLI argument (if provided)
/// 2. `LLMPOOL_CONFIG` environment variable
/// 3. Default: `./llmpool.toml`
pub fn resolve_config_path(cli_path: Option<&str>) -> PathBuf {
    if let Some(p) = cli_path {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("LLMPOOL_CONFIG") {
        return PathBuf::from(p);
    }
    PathBuf::from("llmpool.toml")
}

/// Load configuration from a TOML file and store it in the global singleton.
///
/// This should be called once at application startup. Panics if the file
/// cannot be read or parsed.
pub fn load_config(path: &PathBuf) -> &'static AppConfig {
    CONFIG.get_or_init(|| {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read config file {:?}: {}", path, e));
        let config: AppConfig = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse config file {:?}: {}", path, e));
        config
    })
}

/// Get a reference to the global configuration.
///
/// Panics if `load_config` has not been called yet.
pub fn get_config() -> &'static AppConfig {
    CONFIG
        .get()
        .expect("Configuration not loaded. Call load_config() first.")
}

/// Get the Redis URL.
///
/// Priority:
/// 1. `REDIS_URL` environment variable (if set)
/// 2. Config file `[redis] url` value
///
/// Panics if neither is available or the URL is empty.
pub fn get_redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| {
        let cfg = get_config();
        let url = cfg.redis.url.clone();
        if url.is_empty() {
            panic!(
                "Redis URL not configured. Set REDIS_URL env var or [redis] url in config file."
            );
        }
        url
    })
}
