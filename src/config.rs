use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

#[derive(Clone)]
pub struct Config {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) env_path: PathBuf,
    pub(crate) setup_required: bool,
    pub(crate) database: DatabaseConfig,
    pub(crate) jwt_secret: String,
    pub(crate) registration_enabled: bool,
    pub(crate) mcp_enabled: bool,
    pub(crate) mcp_auth_required: bool,
    pub(crate) embedding_provider: String,
    pub(crate) embedding_model: String,
    pub(crate) siliconflow_url: String,
    pub(crate) siliconflow_api_key: String,
    pub(crate) reranker_enabled: bool,
    pub(crate) reranker_provider: String,
    pub(crate) reranker_model: String,
    pub(crate) reranker_url: String,
    pub(crate) mcp_public_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let env_path = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".env");
        Self::load(env_path, true)
    }

    pub(crate) fn from_file(env_path: &Path) -> Self {
        Self::load(env_path.to_path_buf(), false)
    }

    fn load(env_path: PathBuf, include_process_env: bool) -> Self {
        let setup_required = !env_path.exists();
        let mut file_values = HashMap::new();
        if !setup_required {
            match dotenvy::from_path_iter(&env_path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok((name, value)) => {
                                file_values.insert(name, value);
                            }
                            Err(error) => warn!(
                                "failed to parse .env entry from {}: {error}",
                                env_path.display()
                            ),
                        }
                    }
                }
                Err(error) => {
                    warn!("failed to load .env from {}: {error}", env_path.display());
                }
            }
        }

        let value = |name: &str| {
            if include_process_env {
                env::var(name).ok()
            } else {
                None
            }
            .or_else(|| file_values.get(name).cloned())
        };
        let database_url = value("DATABASE_URL")
            .unwrap_or_else(|| "sqlite://data/guglerag.db?mode=rwc".to_string());
        Self {
            host: value("SERVER_HOST").unwrap_or_else(|| "0.0.0.0".to_string()),
            port: value("SERVER_PORT")
                .and_then(|value| value.parse().ok())
                .unwrap_or(8080),
            env_path,
            setup_required,
            database: DatabaseConfig::from_url(database_url),
            jwt_secret: value("JWT_SECRET")
                .unwrap_or_else(|| "development-only-change-me-before-production".to_string()),
            registration_enabled: parse_env_bool(value("REGISTRATION_ENABLED").as_deref(), true),
            mcp_enabled: parse_env_bool(value("MCP_ENABLED").as_deref(), true),
            mcp_auth_required: parse_env_bool(value("MCP_AUTH_REQUIRED").as_deref(), false),
            embedding_provider: value("EMBEDDING_PROVIDER").unwrap_or_else(|| "stub".to_string()),
            embedding_model: value("EMBEDDING_MODEL").unwrap_or_else(|| "none".to_string()),
            siliconflow_url: value("SILICONFLOW_URL")
                .unwrap_or_else(|| "https://api.siliconflow.cn".to_string()),
            siliconflow_api_key: value("SILICONFLOW_API_KEY").unwrap_or_default(),
            reranker_enabled: parse_env_bool(value("RERANKER_ENABLED").as_deref(), false),
            reranker_provider: value("RERANKER_PROVIDER").unwrap_or_else(|| "none".to_string()),
            reranker_model: value("RERANKER_MODEL")
                .unwrap_or_else(|| "BAAI/bge-reranker-v2-m3".to_string()),
            reranker_url: value("RERANKER_URL").unwrap_or_default(),
            mcp_public_url: value("MCP_PUBLIC_URL").unwrap_or_default(),
        }
    }

    pub(crate) fn for_test(database_url: String, jwt_secret: String) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 0,
            env_path: env::temp_dir().join(format!("guglerag-test-{}.env", Uuid::new_v4())),
            setup_required: false,
            database: DatabaseConfig::from_url(database_url),
            jwt_secret,
            registration_enabled: true,
            mcp_enabled: true,
            mcp_auth_required: true,
            embedding_provider: "stub".to_string(),
            embedding_model: "none".to_string(),
            siliconflow_url: "https://api.siliconflow.cn".to_string(),
            siliconflow_api_key: String::new(),
            reranker_enabled: false,
            reranker_provider: "none".to_string(),
            reranker_model: "none".to_string(),
            reranker_url: String::new(),
            mcp_public_url: String::new(),
        }
    }

    pub(crate) fn same_runtime_settings(&self, other: &Self) -> bool {
        self.host == other.host
            && self.port == other.port
            && self.database == other.database
            && self.jwt_secret == other.jwt_secret
            && self.registration_enabled == other.registration_enabled
            && self.mcp_enabled == other.mcp_enabled
            && self.mcp_auth_required == other.mcp_auth_required
            && self.embedding_provider == other.embedding_provider
            && self.embedding_model == other.embedding_model
            && self.siliconflow_url == other.siliconflow_url
            && self.siliconflow_api_key == other.siliconflow_api_key
            && self.reranker_enabled == other.reranker_enabled
            && self.reranker_provider == other.reranker_provider
            && self.reranker_model == other.reranker_model
            && self.reranker_url == other.reranker_url
            && self.mcp_public_url == other.mcp_public_url
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseEngine {
    Sqlite,
    Mysql,
    Postgres,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub engine: DatabaseEngine,
    pub url: String,
}

impl DatabaseConfig {
    pub fn from_url(url: String) -> Self {
        let engine = if url.starts_with("mysql://") {
            DatabaseEngine::Mysql
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            DatabaseEngine::Postgres
        } else {
            DatabaseEngine::Sqlite
        };
        let url = if matches!(engine, DatabaseEngine::Sqlite) && !url.contains('?') {
            format!("{url}?mode=rwc")
        } else {
            url
        };
        Self { engine, url }
    }

    pub fn redacted_url(&self) -> String {
        redact_database_url(&self.url)
    }
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub server_host: String,
    pub server_port: u16,
    pub database_url: String,
    pub jwt_secret: String,
    #[serde(default = "default_registration_enabled")]
    pub registration_enabled: bool,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub siliconflow_url: Option<String>,
    pub siliconflow_api_key: Option<String>,
    pub reranker_enabled: bool,
    pub reranker_provider: String,
    pub reranker_model: String,
    pub reranker_url: String,
    pub mcp_enabled: bool,
    pub mcp_auth_required: bool,
    #[serde(default)]
    pub mcp_public_url: Option<String>,
}

pub fn validate_setup(input: &SetupRequest) -> Result<(), AppError> {
    if input.server_host.trim().is_empty() {
        return Err(AppError::BadRequest("SERVER_HOST is required".to_string()));
    }
    if input.server_port == 0 {
        return Err(AppError::BadRequest(
            "SERVER_PORT must be a valid TCP port".to_string(),
        ));
    }
    if input.jwt_secret.trim().len() < 32 {
        return Err(AppError::BadRequest(
            "JWT_SECRET must be at least 32 characters".to_string(),
        ));
    }
    validate_database_url(&input.database_url)?;
    match input.embedding_provider.as_str() {
        "stub" | "local" | "siliconflow" => {}
        _ => {
            return Err(AppError::BadRequest(
                "EMBEDDING_PROVIDER must be stub, local, or siliconflow".to_string(),
            ));
        }
    }
    if input.embedding_provider == "siliconflow"
        && input
            .siliconflow_api_key
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(AppError::BadRequest(
            "SILICONFLOW_API_KEY is required for siliconflow embeddings".to_string(),
        ));
    }
    if input.reranker_enabled {
        match input.reranker_provider.as_str() {
            "local" | "siliconflow" | "custom_http" => {}
            _ => {
                return Err(AppError::BadRequest(
                    "RERANKER_PROVIDER must be local, siliconflow, or custom_http".to_string(),
                ));
            }
        }
        if input.reranker_model.trim().is_empty() {
            return Err(AppError::BadRequest(
                "RERANKER_MODEL is required when reranker is enabled".to_string(),
            ));
        }
        if input.reranker_provider == "custom_http" && input.reranker_url.trim().is_empty() {
            return Err(AppError::BadRequest(
                "RERANKER_URL is required for custom_http reranker".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn validate_database_url(url: &str) -> Result<DatabaseEngine, AppError> {
    let url = url.trim();
    if url.starts_with("sqlite://") || url.starts_with("sqlite:") {
        Ok(DatabaseEngine::Sqlite)
    } else if url.starts_with("mysql://") {
        Ok(DatabaseEngine::Mysql)
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(DatabaseEngine::Postgres)
    } else {
        Err(AppError::BadRequest(
            "DATABASE_URL must start with sqlite:, sqlite://, mysql://, postgres://, or postgresql://"
                .to_string(),
        ))
    }
}

pub(crate) async fn prepare_database_path(database: &DatabaseConfig) {
    if !matches!(database.engine, DatabaseEngine::Sqlite) {
        return;
    }
    let path = database
        .url
        .split('?')
        .next()
        .unwrap_or(&database.url)
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:");
    if path == ":memory:" {
        return;
    }
    let path = PathBuf::from(path);
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return;
    };
    if let Err(error) = fs::create_dir_all(parent).await {
        warn!(
            "failed to create sqlite database directory {}: {error}",
            parent.display()
        );
    }
}

pub fn render_env_file(input: &SetupRequest) -> String {
    let siliconflow_url = input
        .siliconflow_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://api.siliconflow.cn");
    let siliconflow_api_key = input
        .siliconflow_api_key
        .as_deref()
        .unwrap_or_default()
        .trim();

    [
        format!("SERVER_HOST={}", env_escape(&input.server_host)),
        format!("SERVER_PORT={}", input.server_port),
        format!("DATABASE_URL={}", env_escape(&input.database_url)),
        format!("JWT_SECRET={}", env_escape(&input.jwt_secret)),
        format!("REGISTRATION_ENABLED={}", input.registration_enabled),
        format!(
            "EMBEDDING_PROVIDER={}",
            env_escape(&input.embedding_provider)
        ),
        format!("EMBEDDING_MODEL={}", env_escape(&input.embedding_model)),
        format!("SILICONFLOW_URL={}", env_escape(siliconflow_url)),
        format!("SILICONFLOW_API_KEY={}", env_escape(siliconflow_api_key)),
        format!("RERANKER_ENABLED={}", input.reranker_enabled),
        format!("RERANKER_PROVIDER={}", env_escape(&input.reranker_provider)),
        format!("RERANKER_MODEL={}", env_escape(&input.reranker_model)),
        format!("RERANKER_URL={}", env_escape(&input.reranker_url)),
        format!("MCP_ENABLED={}", input.mcp_enabled),
        format!("MCP_AUTH_REQUIRED={}", input.mcp_auth_required),
        format!(
            "MCP_PUBLIC_URL={}",
            env_escape(input.mcp_public_url.as_deref().unwrap_or_default())
        ),
        String::new(),
    ]
    .join("\n")
}

fn default_registration_enabled() -> bool {
    true
}

fn env_escape(value: &str) -> String {
    let needs_quotes = value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '#' | '='));
    if needs_quotes {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.trim().to_string()
    }
}

fn redact_database_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    let Some(at_index) = after_scheme.find('@') else {
        return url.to_string();
    };
    let authority = &after_scheme[..at_index];
    let Some(colon_index) = authority.rfind(':') else {
        return url.to_string();
    };
    format!(
        "{}{}:{}@{}",
        &url[..scheme_end + 3],
        &authority[..colon_index],
        "*****",
        &after_scheme[at_index + 1..]
    )
}

fn parse_env_bool(value: Option<&str>, default: bool) -> bool {
    value
        .map(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(default)
}
