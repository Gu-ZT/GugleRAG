use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{AnyPool, Row, any::AnyPoolOptions};
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::{fs, net::TcpListener};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "guglerag=info,tower_http=info".into()),
        )
        .init();

    sqlx::any::install_default_drivers();
    let config = Config::from_env();
    prepare_database_path(&config.database).await;
    let database = Database::connect(&config.database.url)
        .await
        .expect("failed to connect database");
    database
        .migrate()
        .await
        .expect("failed to migrate database");
    let state = AppState {
        database,
        config: Arc::new(config.clone()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup", post(save_setup))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/me", get(me))
        .route("/api/documents", get(list_documents).post(create_document))
        .route(
            "/api/documents/{id}",
            get(read_document)
                .put(update_document)
                .delete(delete_document),
        )
        .route("/api/search", get(search_documents))
        .route("/mcp", post(mcp_endpoint))
        .fallback_service(
            ServeDir::new("frontend/dist")
                .not_found_service(ServeFile::new("frontend/dist/index.html")),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("SERVER_HOST/SERVER_PORT must form a valid socket address");
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind configured address");

    info!("GugleRAG listening on http://{addr}");
    axum::serve(listener, app).await.expect("server failed");
}

#[derive(Clone)]
struct Config {
    host: String,
    port: u16,
    env_path: PathBuf,
    setup_required: bool,
    database: DatabaseConfig,
    jwt_secret: String,
    mcp_enabled: bool,
    mcp_auth_required: bool,
    embedding_provider: String,
    embedding_model: String,
    siliconflow_url: String,
    reranker_enabled: bool,
    reranker_provider: String,
    reranker_model: String,
    reranker_url: String,
}

impl Config {
    fn from_env() -> Self {
        let env_path = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".env");
        let setup_required = !env_path.exists();
        if !setup_required {
            if let Err(error) = dotenvy::from_path(&env_path) {
                warn!("failed to load .env from {}: {error}", env_path.display());
            }
        }

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/guglerag.db?mode=rwc".to_string());
        let database = DatabaseConfig::from_url(database_url);

        Self {
            host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("SERVER_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(8080),
            env_path,
            setup_required,
            database,
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "development-only-change-me-before-production".to_string()),
            mcp_enabled: env_bool("MCP_ENABLED", true),
            mcp_auth_required: env_bool("MCP_AUTH_REQUIRED", false),
            embedding_provider: env::var("EMBEDDING_PROVIDER")
                .unwrap_or_else(|_| "stub".to_string()),
            embedding_model: env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "none".to_string()),
            siliconflow_url: env::var("SILICONFLOW_URL")
                .unwrap_or_else(|_| "https://api.siliconflow.cn".to_string()),
            reranker_enabled: env_bool("RERANKER_ENABLED", false),
            reranker_provider: env::var("RERANKER_PROVIDER").unwrap_or_else(|_| "none".to_string()),
            reranker_model: env::var("RERANKER_MODEL")
                .unwrap_or_else(|_| "BAAI/bge-reranker-v2-m3".to_string()),
            reranker_url: env::var("RERANKER_URL").unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum DatabaseEngine {
    Sqlite,
    Mysql,
    Postgres,
}

#[derive(Debug, Clone, Serialize)]
struct DatabaseConfig {
    engine: DatabaseEngine,
    url: String,
}

impl DatabaseConfig {
    fn from_url(url: String) -> Self {
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

    fn redacted_url(&self) -> String {
        redact_database_url(&self.url)
    }
}

fn validate_setup(input: &SetupRequest) -> Result<(), AppError> {
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

fn validate_database_url(url: &str) -> Result<DatabaseEngine, AppError> {
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

async fn prepare_database_path(database: &DatabaseConfig) {
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
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent).await {
                warn!(
                    "failed to create sqlite database directory {}: {error}",
                    parent.display()
                );
            }
        }
    }
}

fn render_env_file(input: &SetupRequest) -> String {
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
        String::new(),
    ]
    .join("\n")
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

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(default)
}

#[derive(Clone)]
struct AppState {
    database: Database,
    config: Arc<Config>,
}

#[derive(Clone)]
struct Database {
    pool: AnyPool,
}

impl Database {
    async fn connect(url: &str) -> Result<Self, AppError> {
        let pool = AnyPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    async fn migrate(&self) -> Result<(), AppError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_users (
                id VARCHAR(36) PRIMARY KEY,
                username VARCHAR(80) NOT NULL UNIQUE,
                display_name VARCHAR(120) NOT NULL,
                password_hash VARCHAR(128) NOT NULL,
                salt VARCHAR(64) NOT NULL,
                role VARCHAR(16) NOT NULL,
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS documents (
                id VARCHAR(36) PRIMARY KEY,
                title VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                parent_id VARCHAR(36),
                tags TEXT NOT NULL,
                author_id VARCHAR(36) NOT NULL,
                created_at VARCHAR(40) NOT NULL,
                updated_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS document_versions (
                id VARCHAR(36) PRIMARY KEY,
                document_id VARCHAR(36) NOT NULL,
                content TEXT NOT NULL,
                saved_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn user_count(&self) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM app_users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<i64, _>("count")?)
    }

    async fn find_user_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    async fn get_user(&self, id: Uuid) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    async fn insert_user(&self, user: &User) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO app_users
             (id, username, display_name, password_hash, salt, role, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user.id.to_string())
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(&user.salt)
        .bind(role_to_str(user.role))
        .bind(user.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_documents(&self, parent_id: Option<Uuid>) -> Result<Vec<Document>, AppError> {
        let rows = if let Some(parent_id) = parent_id {
            sqlx::query(
                "SELECT id, title, content, parent_id, tags, author_id, created_at, updated_at
                 FROM documents WHERE parent_id = ? ORDER BY LOWER(title)",
            )
            .bind(parent_id.to_string())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, title, content, parent_id, tags, author_id, created_at, updated_at
                 FROM documents WHERE parent_id IS NULL ORDER BY LOWER(title)",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter()
            .map(row_to_document_without_versions)
            .collect()
    }

    async fn all_documents(&self) -> Result<Vec<Document>, AppError> {
        let rows = sqlx::query(
            "SELECT id, title, content, parent_id, tags, author_id, created_at, updated_at
             FROM documents",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(row_to_document_without_versions)
            .collect()
    }

    async fn get_document(&self, id: Uuid) -> Result<Option<Document>, AppError> {
        let row = sqlx::query(
            "SELECT id, title, content, parent_id, tags, author_id, created_at, updated_at
             FROM documents WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let mut doc = row_to_document_without_versions(row)?;
        doc.versions = self.document_versions(id).await?;
        Ok(Some(doc))
    }

    async fn document_exists(&self, id: Uuid) -> Result<bool, AppError> {
        let row = sqlx::query("SELECT 1 AS exists_flag FROM documents WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    async fn insert_document(&self, doc: &Document) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO documents
             (id, title, content, parent_id, tags, author_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(doc.id.to_string())
        .bind(&doc.title)
        .bind(&doc.content)
        .bind(doc.parent_id.map(|value| value.to_string()))
        .bind(serde_json::to_string(&doc.tags)?)
        .bind(doc.author_id.to_string())
        .bind(doc.created_at.to_rfc3339())
        .bind(doc.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_document(&self, doc: &Document) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE documents
             SET title = ?, content = ?, parent_id = ?, tags = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&doc.title)
        .bind(&doc.content)
        .bind(doc.parent_id.map(|value| value.to_string()))
        .bind(serde_json::to_string(&doc.tags)?)
        .bind(doc.updated_at.to_rfc3339())
        .bind(doc.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_document_version(
        &self,
        document_id: Uuid,
        version: &DocumentVersion,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO document_versions (id, document_id, content, saved_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(document_id.to_string())
        .bind(&version.content)
        .bind(version.saved_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_document_tree(&self, id: Uuid) -> Result<(), AppError> {
        if !self.document_exists(id).await? {
            return Err(AppError::NotFound("document not found".to_string()));
        }

        let mut stack = vec![id];
        let mut ordered = Vec::new();
        while let Some(current_id) = stack.pop() {
            ordered.push(current_id);
            let child_rows = sqlx::query("SELECT id FROM documents WHERE parent_id = ?")
                .bind(current_id.to_string())
                .fetch_all(&self.pool)
                .await?;
            for row in child_rows {
                stack.push(parse_uuid_str(row.try_get::<String, _>("id")?)?);
            }
        }

        for document_id in ordered.into_iter().rev() {
            sqlx::query("DELETE FROM document_versions WHERE document_id = ?")
                .bind(document_id.to_string())
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM documents WHERE id = ?")
                .bind(document_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn document_versions(&self, document_id: Uuid) -> Result<Vec<DocumentVersion>, AppError> {
        let rows = sqlx::query(
            "SELECT content, saved_at FROM document_versions
             WHERE document_id = ? ORDER BY saved_at DESC",
        )
        .bind(document_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_document_version).collect()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: Uuid,
    username: String,
    display_name: String,
    password_hash: String,
    salt: String,
    role: Role,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Role {
    Admin,
    Editor,
    Reader,
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Reader => "reader",
    }
}

fn parse_role(value: &str) -> Result<Role, AppError> {
    match value {
        "admin" => Ok(Role::Admin),
        "editor" => Ok(Role::Editor),
        "reader" => Ok(Role::Reader),
        _ => Err(AppError::Internal(format!("unknown role: {value}"))),
    }
}

fn parse_uuid_str(value: String) -> Result<Uuid, AppError> {
    value
        .parse()
        .map_err(|_| AppError::Internal("invalid uuid in database".to_string()))
}

fn parse_datetime_str(value: String) -> Result<DateTime<Utc>, AppError> {
    Ok(DateTime::parse_from_rfc3339(&value)
        .map_err(|error| AppError::Internal(error.to_string()))?
        .with_timezone(&Utc))
}

fn row_to_user(row: sqlx::any::AnyRow) -> Result<User, AppError> {
    Ok(User {
        id: parse_uuid_str(row.try_get::<String, _>("id")?)?,
        username: row.try_get("username")?,
        display_name: row.try_get("display_name")?,
        password_hash: row.try_get("password_hash")?,
        salt: row.try_get("salt")?,
        role: parse_role(row.try_get::<String, _>("role")?.as_str())?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}

fn row_to_document_without_versions(row: sqlx::any::AnyRow) -> Result<Document, AppError> {
    let tags_json: String = row.try_get("tags")?;
    Ok(Document {
        id: parse_uuid_str(row.try_get::<String, _>("id")?)?,
        title: row.try_get("title")?,
        content: row.try_get("content")?,
        parent_id: row
            .try_get::<Option<String>, _>("parent_id")?
            .map(parse_uuid_str)
            .transpose()?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        author_id: parse_uuid_str(row.try_get::<String, _>("author_id")?)?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
        updated_at: parse_datetime_str(row.try_get("updated_at")?)?,
        versions: Vec::new(),
    })
}

fn row_to_document_version(row: sqlx::any::AnyRow) -> Result<DocumentVersion, AppError> {
    Ok(DocumentVersion {
        content: row.try_get("content")?,
        saved_at: parse_datetime_str(row.try_get("saved_at")?)?,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Document {
    id: Uuid,
    title: String,
    content: String,
    parent_id: Option<Uuid>,
    tags: Vec<String>,
    author_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    versions: Vec<DocumentVersion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DocumentVersion {
    content: String,
    saved_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct PublicUser {
    id: Uuid,
    username: String,
    display_name: String,
    role: Role,
    created_at: DateTime<Utc>,
}

impl From<&User> for PublicUser {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            role: user.role,
            created_at: user.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    token: String,
    user: PublicUser,
}

#[derive(Debug, Deserialize)]
struct DocumentRequest {
    title: Option<String>,
    content: Option<String>,
    parent_id: Option<Uuid>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SetupRequest {
    server_host: String,
    server_port: u16,
    database_url: String,
    jwt_secret: String,
    embedding_provider: String,
    embedding_model: String,
    siliconflow_url: Option<String>,
    siliconflow_api_key: Option<String>,
    reranker_enabled: bool,
    reranker_provider: String,
    reranker_model: String,
    reranker_url: String,
    mcp_enabled: bool,
    mcp_auth_required: bool,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    id: Uuid,
    title: String,
    excerpt: String,
    score: usize,
    updated_at: DateTime<Utc>,
}

#[derive(Debug)]
enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::Unauthorized(message) => (StatusCode::UNAUTHORIZED, message),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message),
            AppError::Conflict(message) => (StatusCode::CONFLICT, message),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError::Internal(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        AppError::Internal(error.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        AppError::Internal(error.to_string())
    }
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "GugleRAG",
        "setup_required": state.config.setup_required,
        "database": {
            "engine": &state.config.database.engine,
            "url": state.config.database.redacted_url()
        },
        "mcp_enabled": state.config.mcp_enabled,
        "embedding": {
            "provider": state.config.embedding_provider,
            "model": state.config.embedding_model,
            "siliconflow_url": state.config.siliconflow_url
        },
        "reranker": {
            "enabled": state.config.reranker_enabled,
            "provider": state.config.reranker_provider,
            "model": state.config.reranker_model,
            "url": state.config.reranker_url
        }
    }))
}

async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "setup_required": state.config.setup_required,
        "env_path": state.config.env_path,
        "supported_databases": ["sqlite", "mysql", "postgres"],
        "current": {
            "server_host": state.config.host,
            "server_port": state.config.port,
            "database": {
                "engine": &state.config.database.engine,
                "url": state.config.database.redacted_url()
            },
            "mcp_enabled": state.config.mcp_enabled,
            "mcp_auth_required": state.config.mcp_auth_required,
            "embedding_provider": state.config.embedding_provider,
            "embedding_model": state.config.embedding_model,
            "reranker_enabled": state.config.reranker_enabled,
            "reranker_provider": state.config.reranker_provider,
            "reranker_model": state.config.reranker_model,
            "reranker_url": state.config.reranker_url
        }
    }))
}

async fn save_setup(
    State(state): State<AppState>,
    Json(input): Json<SetupRequest>,
) -> Result<Json<Value>, AppError> {
    if !state.config.setup_required {
        return Err(AppError::Conflict(
            ".env already exists; edit it directly and restart the server".to_string(),
        ));
    }
    validate_setup(&input)?;
    let env_content = render_env_file(&input);
    fs::write(&state.config.env_path, env_content).await?;
    Ok(Json(json!({
        "ok": true,
        "env_path": state.config.env_path,
        "restart_required": true
    })))
}

async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    validate_username(&input.username)?;
    validate_password(&input.password)?;

    if state
        .database
        .find_user_by_username(input.username.trim())
        .await?
        .is_some()
    {
        return Err(AppError::Conflict("username already exists".to_string()));
    }

    let id = Uuid::new_v4();
    let salt = Uuid::new_v4().to_string();
    let role = if state.database.user_count().await? == 0 {
        Role::Admin
    } else {
        Role::Editor
    };
    let user = User {
        id,
        username: input.username.trim().to_string(),
        display_name: input
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| input.username.trim().to_string()),
        password_hash: hash_password(&salt, &input.password),
        salt,
        role,
        created_at: Utc::now(),
    };

    state.database.insert_user(&user).await?;
    let token = issue_token(id, &state.config.jwt_secret)?;
    Ok(Json(AuthResponse {
        token,
        user: PublicUser::from(&user),
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user = state
        .database
        .find_user_by_username(input.username.trim())
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid username or password".to_string()))?;

    if user.password_hash != hash_password(&user.salt, &input.password) {
        return Err(AppError::Unauthorized(
            "invalid username or password".to_string(),
        ));
    }

    let token = issue_token(user.id, &state.config.jwt_secret)?;
    Ok(Json(AuthResponse {
        token,
        user: PublicUser::from(&user),
    }))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicUser>, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    Ok(Json(PublicUser::from(&user)))
}

async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    require_user(&headers, &state).await?;
    Ok(Json(state.database.list_documents(query.parent_id).await?))
}

async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let title = require_non_empty(input.title, "title")?;
    let content = input.content.unwrap_or_default();
    let now = Utc::now();
    let doc = Document {
        id: Uuid::new_v4(),
        title,
        content,
        parent_id: input.parent_id,
        tags: input.tags.unwrap_or_default(),
        author_id: user_id,
        created_at: now,
        updated_at: now,
        versions: Vec::new(),
    };

    if let Some(parent_id) = doc.parent_id {
        if !state.database.document_exists(parent_id).await? {
            return Err(AppError::BadRequest("parent_id does not exist".to_string()));
        }
    }
    state.database.insert_document(&doc).await?;
    Ok(Json(doc))
}

async fn read_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Document>, AppError> {
    require_user(&headers, &state).await?;
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    Ok(Json(doc))
}

async fn update_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !matches!(user.role, Role::Admin | Role::Editor) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    if let Some(parent_id) = input.parent_id {
        if parent_id == id {
            return Err(AppError::BadRequest(
                "document cannot be its own parent".to_string(),
            ));
        }
        if !state.database.document_exists(parent_id).await? {
            return Err(AppError::BadRequest("parent_id does not exist".to_string()));
        }
    }
    let mut doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;

    if input.content.is_some() {
        let version = DocumentVersion {
            content: doc.content.clone(),
            saved_at: Utc::now(),
        };
        state.database.insert_document_version(id, &version).await?;
        doc.versions.push(version);
    }
    if let Some(title) = input.title {
        doc.title = require_non_empty(Some(title), "title")?;
    }
    if let Some(content) = input.content {
        doc.content = content;
    }
    if input.parent_id.is_some() {
        doc.parent_id = input.parent_id;
    }
    if let Some(tags) = input.tags {
        doc.tags = tags;
    }
    doc.updated_at = Utc::now();

    let updated = doc.clone();
    state.database.update_document(&updated).await?;
    Ok(Json(updated))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !matches!(user.role, Role::Admin | Role::Editor) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    state.database.delete_document_tree(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn search_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    require_user(&headers, &state).await?;
    let documents = state.database.all_documents().await?;
    Ok(Json(search_documents_in_memory(
        &documents,
        &query.q,
        query.limit.unwrap_or(10),
    )))
}

fn search_documents_in_memory(
    documents: &[Document],
    query: &str,
    limit: usize,
) -> Vec<SearchResult> {
    let terms = query
        .to_lowercase()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if terms.is_empty() {
        return Vec::new();
    }

    let mut results = documents
        .iter()
        .filter_map(|doc| {
            let title = doc.title.to_lowercase();
            let content = doc.content.to_lowercase();
            let tags = doc.tags.join(" ").to_lowercase();
            let score = terms.iter().fold(0usize, |score, term| {
                score
                    + title.matches(term).count() * 8
                    + tags.matches(term).count() * 5
                    + content.matches(term).count()
            });
            (score > 0).then(|| SearchResult {
                id: doc.id,
                title: doc.title.clone(),
                excerpt: excerpt(&doc.content, &terms),
                score,
                updated_at: doc.updated_at,
            })
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| b.score.cmp(&a.score).then(b.updated_at.cmp(&a.updated_at)));
    results.truncate(limit.min(50));
    results
}

fn excerpt(content: &str, terms: &[String]) -> String {
    let lower = content.to_lowercase();
    let start = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0);
    let char_start = content
        .char_indices()
        .take_while(|(idx, _)| *idx < start)
        .count()
        .saturating_sub(40);
    content
        .chars()
        .skip(char_start)
        .take(160)
        .collect::<String>()
        .replace('\n', " ")
}

async fn mcp_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !state.config.mcp_enabled {
        return Json(json_rpc_error(id, -32000, "MCP endpoint disabled"));
    }
    if state.config.mcp_auth_required && require_user(&headers, &state).await.is_err() {
        return Json(json_rpc_error(id, -32001, "authentication required"));
    }

    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": "GugleRAG", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "tools": {} }
        }),
        "tools/list" => json!({ "tools": mcp_tools() }),
        "tools/call" => match call_mcp_tool(&state, params).await {
            Ok(value) => value,
            Err(error) => return Json(json_rpc_error(id, -32602, &error)),
        },
        _ => return Json(json_rpc_error(id, -32601, "method not found")),
    };

    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn mcp_tools() -> Value {
    json!([
        {
            "name": "search_knowledge",
            "description": "Search the team knowledge base.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "read_document",
            "description": "Read a document by id.",
            "inputSchema": {
                "type": "object",
                "properties": { "doc_id": { "type": "string" } },
                "required": ["doc_id"]
            }
        },
        {
            "name": "create_document",
            "description": "Create a Markdown document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "parent_id": { "type": "string" }
                },
                "required": ["title", "content"]
            }
        },
        {
            "name": "update_document",
            "description": "Update a document title/content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string" },
                    "title": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["doc_id"]
            }
        },
        {
            "name": "list_documents",
            "description": "List documents under a folder id.",
            "inputSchema": {
                "type": "object",
                "properties": { "folder_id": { "type": "string" } }
            }
        },
        {
            "name": "get_document_metadata",
            "description": "Get document metadata without full content.",
            "inputSchema": {
                "type": "object",
                "properties": { "doc_id": { "type": "string" } },
                "required": ["doc_id"]
            }
        }
    ])
}

async fn call_mcp_tool(state: &AppState, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let value = match name {
        "search_knowledge" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing query".to_string())?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
            let documents = state
                .database
                .all_documents()
                .await
                .map_err(|error| format!("{error:?}"))?;
            json!(search_documents_in_memory(&documents, query, limit))
        }
        "read_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            json!(
                state
                    .database
                    .get_document(doc_id)
                    .await
                    .map_err(|error| format!("{error:?}"))?
                    .ok_or_else(|| "document not found".to_string())?
            )
        }
        "create_document" => {
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing title".to_string())?
                .trim()
                .to_string();
            if title.is_empty() {
                return Err("title cannot be empty".to_string());
            }
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let parent_id = optional_uuid_arg(&args, "parent_id")?;
            if let Some(parent_id) = parent_id {
                if !state
                    .database
                    .document_exists(parent_id)
                    .await
                    .map_err(|error| format!("{error:?}"))?
                {
                    return Err("parent_id does not exist".to_string());
                }
            }
            let author_id = system_user(&state.database)
                .await
                .map_err(|error| format!("{error:?}"))?;
            let now = Utc::now();
            let doc = Document {
                id: Uuid::new_v4(),
                title,
                content,
                parent_id,
                tags: Vec::new(),
                author_id,
                created_at: now,
                updated_at: now,
                versions: Vec::new(),
            };
            state
                .database
                .insert_document(&doc)
                .await
                .map_err(|error| format!("{error:?}"))?;
            json!(doc)
        }
        "update_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let mut doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|error| format!("{error:?}"))?
                .ok_or_else(|| "document not found".to_string())?;
            if let Some(title) = args.get("title").and_then(Value::as_str) {
                if title.trim().is_empty() {
                    return Err("title cannot be empty".to_string());
                }
                doc.title = title.trim().to_string();
            }
            if let Some(content) = args.get("content").and_then(Value::as_str) {
                let version = DocumentVersion {
                    content: doc.content.clone(),
                    saved_at: Utc::now(),
                };
                state
                    .database
                    .insert_document_version(doc_id, &version)
                    .await
                    .map_err(|error| format!("{error:?}"))?;
                doc.versions.push(version);
                doc.content = content.to_string();
            }
            doc.updated_at = Utc::now();
            let updated = doc.clone();
            state
                .database
                .update_document(&updated)
                .await
                .map_err(|error| format!("{error:?}"))?;
            json!(updated)
        }
        "list_documents" => {
            let folder_id = optional_uuid_arg(&args, "folder_id")?;
            let docs = state
                .database
                .list_documents(folder_id)
                .await
                .map_err(|error| format!("{error:?}"))?
                .iter()
                .map(|doc| document_metadata(doc))
                .collect::<Vec<_>>();
            json!(docs)
        }
        "get_document_metadata" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let doc = state
                .database
                .get_document(doc_id)
                .await
                .map_err(|error| format!("{error:?}"))?
                .ok_or_else(|| "document not found".to_string())?;
            document_metadata(&doc)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };

    Ok(json!({ "content": [{ "type": "text", "text": value.to_string() }] }))
}

fn document_metadata(doc: &Document) -> Value {
    json!({
        "id": doc.id,
        "title": doc.title,
        "parent_id": doc.parent_id,
        "tags": doc.tags,
        "author_id": doc.author_id,
        "created_at": doc.created_at,
        "updated_at": doc.updated_at,
        "version_count": doc.versions.len()
    })
}

fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn parse_uuid_arg(args: &Value, name: &str) -> Result<Uuid, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {name}"))?
        .parse()
        .map_err(|_| format!("{name} must be a uuid"))
}

fn optional_uuid_arg(args: &Value, name: &str) -> Result<Option<Uuid>, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.parse().map_err(|_| format!("{name} must be a uuid")))
        .transpose()
}

async fn system_user(database: &Database) -> Result<Uuid, AppError> {
    if let Some(user) = database.find_user_by_username("mcp-system").await? {
        return Ok(user.id);
    }
    let id = Uuid::new_v4();
    let salt = Uuid::new_v4().to_string();
    database
        .insert_user(&User {
            id,
            username: "mcp-system".to_string(),
            display_name: "MCP System".to_string(),
            password_hash: hash_password(&salt, &Uuid::new_v4().to_string()),
            salt,
            role: Role::Admin,
            created_at: Utc::now(),
        })
        .await?;
    Ok(id)
}

async fn require_user(headers: &HeaderMap, state: &AppState) -> Result<Uuid, AppError> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("missing bearer token".to_string()))?;
    let token = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid bearer token".to_string()))?;
    let user_id = token
        .claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized("invalid token subject".to_string()))?;
    Ok(user_id)
}

fn issue_token(user_id: Uuid, secret: &str) -> Result<String, AppError> {
    let claims = Claims {
        sub: user_id.to_string(),
        exp: (Utc::now() + Duration::days(7)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|error| AppError::Internal(error.to_string()))
}

fn hash_password(salt: &str, password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(password.as_bytes());
    STANDARD_NO_PAD.encode(hasher.finalize())
}

fn validate_username(username: &str) -> Result<(), AppError> {
    let username = username.trim();
    if username.len() < 3 || username.len() > 40 {
        return Err(AppError::BadRequest(
            "username must be 3-40 characters".to_string(),
        ));
    }
    if !username
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(AppError::BadRequest(
            "username may contain only letters, numbers, '_' and '-'".to_string(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

fn require_non_empty(value: Option<String>, field: &str) -> Result<String, AppError> {
    let value = value.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest(format!("{field} is required")));
    }
    Ok(value)
}
