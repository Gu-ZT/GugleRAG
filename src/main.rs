use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::{fs, net::TcpListener, sync::RwLock};
use tower_http::trace::TraceLayer;
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

    let config = Config::from_env();
    let store = Store::load(&config.data_path).await.unwrap_or_default();
    let state = AppState {
        store: Arc::new(RwLock::new(store)),
        config: Arc::new(config.clone()),
    };

    let app = Router::new()
        .route("/", get(index))
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
    data_path: PathBuf,
    jwt_secret: String,
    mcp_enabled: bool,
    mcp_auth_required: bool,
    embedding_provider: String,
    embedding_model: String,
    siliconflow_url: String,
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

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/guglerag.db".to_string());
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
            data_path: env::var("GUGLERAG_DATA")
                .unwrap_or_else(|_| "data.json".to_string())
                .into(),
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "development-only-change-me-before-production".to_string()),
            mcp_enabled: env_bool("MCP_ENABLED", true),
            mcp_auth_required: env_bool("MCP_AUTH_REQUIRED", false),
            embedding_provider: env::var("EMBEDDING_PROVIDER")
                .unwrap_or_else(|_| "stub".to_string()),
            embedding_model: env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "none".to_string()),
            siliconflow_url: env::var("SILICONFLOW_URL")
                .unwrap_or_else(|_| "https://api.siliconflow.cn".to_string()),
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
        "GUGLERAG_DATA=data.json".to_string(),
        format!("JWT_SECRET={}", env_escape(&input.jwt_secret)),
        format!(
            "EMBEDDING_PROVIDER={}",
            env_escape(&input.embedding_provider)
        ),
        format!("EMBEDDING_MODEL={}", env_escape(&input.embedding_model)),
        format!("SILICONFLOW_URL={}", env_escape(siliconflow_url)),
        format!("SILICONFLOW_API_KEY={}", env_escape(siliconflow_api_key)),
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
    store: Arc<RwLock<Store>>,
    config: Arc<Config>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Store {
    users: HashMap<Uuid, User>,
    documents: HashMap<Uuid, Document>,
}

impl Store {
    async fn load(path: &PathBuf) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn save(&self, path: &PathBuf) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(path, bytes).await?;
        Ok(())
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

async fn index(State(state): State<AppState>) -> Html<&'static str> {
    if state.config.setup_required {
        Html(SETUP_HTML)
    } else {
        Html(INDEX_HTML)
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
            "embedding_model": state.config.embedding_model
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

    let mut store = state.store.write().await;
    if store
        .users
        .values()
        .any(|user| user.username == input.username)
    {
        return Err(AppError::Conflict("username already exists".to_string()));
    }

    let id = Uuid::new_v4();
    let salt = Uuid::new_v4().to_string();
    let role = if store.users.is_empty() {
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

    store.users.insert(id, user.clone());
    store.save(&state.config.data_path).await?;
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
    let store = state.store.read().await;
    let user = store
        .users
        .values()
        .find(|user| user.username == input.username)
        .ok_or_else(|| AppError::Unauthorized("invalid username or password".to_string()))?;

    if user.password_hash != hash_password(&user.salt, &input.password) {
        return Err(AppError::Unauthorized(
            "invalid username or password".to_string(),
        ));
    }

    let token = issue_token(user.id, &state.config.jwt_secret)?;
    Ok(Json(AuthResponse {
        token,
        user: PublicUser::from(user),
    }))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicUser>, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let store = state.store.read().await;
    let user = store
        .users
        .get(&user_id)
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    Ok(Json(PublicUser::from(user)))
}

async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    require_user(&headers, &state).await?;
    let store = state.store.read().await;
    let mut docs = store
        .documents
        .values()
        .filter(|doc| doc.parent_id == query.parent_id)
        .cloned()
        .collect::<Vec<_>>();
    docs.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(Json(docs))
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

    let mut store = state.store.write().await;
    if let Some(parent_id) = doc.parent_id {
        if !store.documents.contains_key(&parent_id) {
            return Err(AppError::BadRequest("parent_id does not exist".to_string()));
        }
    }
    store.documents.insert(doc.id, doc.clone());
    store.save(&state.config.data_path).await?;
    Ok(Json(doc))
}

async fn read_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Document>, AppError> {
    require_user(&headers, &state).await?;
    let store = state.store.read().await;
    let doc = store
        .documents
        .get(&id)
        .cloned()
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
    let mut store = state.store.write().await;
    if !matches!(
        current_role(user_id, &store),
        Some(Role::Admin | Role::Editor)
    ) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    if let Some(parent_id) = input.parent_id {
        if parent_id == id {
            return Err(AppError::BadRequest(
                "document cannot be its own parent".to_string(),
            ));
        }
        if !store.documents.contains_key(&parent_id) {
            return Err(AppError::BadRequest("parent_id does not exist".to_string()));
        }
    }
    let doc = store
        .documents
        .get_mut(&id)
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;

    if input.content.is_some() {
        doc.versions.push(DocumentVersion {
            content: doc.content.clone(),
            saved_at: Utc::now(),
        });
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
    store.save(&state.config.data_path).await?;
    Ok(Json(updated))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_user(&headers, &state).await?;
    let mut store = state.store.write().await;
    if !matches!(
        current_role(user_id, &store),
        Some(Role::Admin | Role::Editor)
    ) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    delete_document_tree(&mut store.documents, id)?;
    store.save(&state.config.data_path).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn search_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    require_user(&headers, &state).await?;
    let store = state.store.read().await;
    Ok(Json(search_store(
        &store,
        &query.q,
        query.limit.unwrap_or(10),
    )))
}

fn delete_document_tree(documents: &mut HashMap<Uuid, Document>, id: Uuid) -> Result<(), AppError> {
    if !documents.contains_key(&id) {
        return Err(AppError::NotFound("document not found".to_string()));
    }
    let child_ids = documents
        .values()
        .filter(|doc| doc.parent_id == Some(id))
        .map(|doc| doc.id)
        .collect::<Vec<_>>();
    for child_id in child_ids {
        delete_document_tree(documents, child_id)?;
    }
    documents.remove(&id);
    Ok(())
}

fn current_role(user_id: Uuid, store: &Store) -> Option<Role> {
    store.users.get(&user_id).map(|user| user.role)
}

fn search_store(store: &Store, query: &str, limit: usize) -> Vec<SearchResult> {
    let terms = query
        .to_lowercase()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if terms.is_empty() {
        return Vec::new();
    }

    let mut results = store
        .documents
        .values()
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
            let store = state.store.read().await;
            json!(search_store(&store, query, limit))
        }
        "read_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let store = state.store.read().await;
            json!(
                store
                    .documents
                    .get(&doc_id)
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
            let mut store = state.store.write().await;
            let author_id = system_user(&mut store);
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
            store.documents.insert(doc.id, doc.clone());
            store
                .save(&state.config.data_path)
                .await
                .map_err(|error| format!("{error:?}"))?;
            json!(doc)
        }
        "update_document" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let mut store = state.store.write().await;
            let doc = store
                .documents
                .get_mut(&doc_id)
                .ok_or_else(|| "document not found".to_string())?;
            if let Some(title) = args.get("title").and_then(Value::as_str) {
                if title.trim().is_empty() {
                    return Err("title cannot be empty".to_string());
                }
                doc.title = title.trim().to_string();
            }
            if let Some(content) = args.get("content").and_then(Value::as_str) {
                doc.versions.push(DocumentVersion {
                    content: doc.content.clone(),
                    saved_at: Utc::now(),
                });
                doc.content = content.to_string();
            }
            doc.updated_at = Utc::now();
            let updated = doc.clone();
            store
                .save(&state.config.data_path)
                .await
                .map_err(|error| format!("{error:?}"))?;
            json!(updated)
        }
        "list_documents" => {
            let folder_id = optional_uuid_arg(&args, "folder_id")?;
            let store = state.store.read().await;
            let mut docs = store
                .documents
                .values()
                .filter(|doc| doc.parent_id == folder_id)
                .map(|doc| document_metadata(doc))
                .collect::<Vec<_>>();
            docs.sort_by_key(|doc| {
                doc.get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_lowercase()
            });
            json!(docs)
        }
        "get_document_metadata" => {
            let doc_id = parse_uuid_arg(&args, "doc_id")?;
            let store = state.store.read().await;
            let doc = store
                .documents
                .get(&doc_id)
                .ok_or_else(|| "document not found".to_string())?;
            document_metadata(doc)
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

fn system_user(store: &mut Store) -> Uuid {
    if let Some(user) = store
        .users
        .values()
        .find(|user| user.username == "mcp-system")
    {
        return user.id;
    }
    let id = Uuid::new_v4();
    let salt = Uuid::new_v4().to_string();
    store.users.insert(
        id,
        User {
            id,
            username: "mcp-system".to_string(),
            display_name: "MCP System".to_string(),
            password_hash: hash_password(&salt, &Uuid::new_v4().to_string()),
            salt,
            role: Role::Admin,
            created_at: Utc::now(),
        },
    );
    id
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

const SETUP_HTML: &str = r###"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>GugleRAG Setup</title>
  <style>
    :root {
      --bg: #f3f6f8;
      --panel: #ffffff;
      --ink: #16212b;
      --muted: #627284;
      --line: #d6dde6;
      --accent: #116a7b;
      --accent-soft: #d9edf1;
      --danger: #b42318;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--ink); }
    .page { min-height: 100vh; display: grid; grid-template-columns: minmax(280px, 420px) minmax(0, 1fr); }
    .rail {
      padding: 38px;
      background: #142632;
      color: #f8fbfc;
      display: grid;
      align-content: space-between;
      gap: 32px;
    }
    .mark { font-size: 28px; font-weight: 800; letter-spacing: .02em; }
    .rail p { color: #b9c7d0; line-height: 1.6; max-width: 31rem; }
    .dial {
      width: 160px;
      aspect-ratio: 1;
      border: 1px solid rgba(255,255,255,.22);
      border-radius: 50%;
      position: relative;
      background:
        conic-gradient(from 35deg, #62c7d8 0 74deg, transparent 74deg 128deg, #f4c95d 128deg 196deg, transparent 196deg 360deg);
      box-shadow: inset 0 0 0 28px #142632;
    }
    .dial::after {
      content: "";
      position: absolute;
      inset: 54px;
      border-radius: 50%;
      background: #f8fbfc;
    }
    main { padding: 34px; display: grid; align-content: center; }
    form {
      width: min(760px, 100%);
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 24px;
      display: grid;
      gap: 18px;
      box-shadow: 0 14px 40px rgba(22,33,43,.08);
    }
    h1 { margin: 0; font-size: 24px; }
    fieldset { border: 0; padding: 0; margin: 0; display: grid; gap: 12px; }
    legend { font-weight: 750; margin-bottom: 10px; }
    .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
    label { display: grid; gap: 6px; color: var(--muted); font-size: 13px; }
    input, select {
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 6px;
      padding: 10px 11px;
      color: var(--ink);
      background: white;
      font: inherit;
    }
    .check { display: flex; align-items: center; gap: 8px; color: var(--ink); }
    .check input { width: auto; }
    .actions { display: flex; justify-content: space-between; align-items: center; gap: 10px; }
    button {
      border: 1px solid var(--accent);
      border-radius: 6px;
      background: var(--accent);
      color: white;
      padding: 10px 14px;
      font: inherit;
      cursor: pointer;
    }
    .hint { color: var(--muted); font-size: 13px; line-height: 1.5; }
    .message { min-height: 20px; font-size: 13px; }
    .message.error { color: var(--danger); }
    .message.ok { color: var(--accent); }
    @media (max-width: 840px) {
      .page { display: block; }
      .rail { padding: 24px; }
      .dial { display: none; }
      main { padding: 18px; }
      .grid { grid-template-columns: 1fr; }
      .actions { align-items: stretch; flex-direction: column; }
      button { width: 100%; }
    }
  </style>
</head>
<body>
  <div class="page">
    <aside class="rail">
      <div>
        <div class="mark">GugleRAG</div>
        <p>首次启动需要写入 .env。这里设置监听地址、数据库连接、JWT 密钥和 MCP 开关；保存后重启服务即可进入知识库。</p>
      </div>
      <div class="dial" aria-hidden="true"></div>
    </aside>
    <main>
      <form id="setup">
        <div>
          <h1>初始化运行配置</h1>
          <div class="hint">支持 SQLite、MySQL、PostgreSQL。开发阶段建议从 SQLite 开始。</div>
        </div>
        <fieldset>
          <legend>服务</legend>
          <div class="grid">
            <label>监听地址<input name="server_host" value="0.0.0.0"></label>
            <label>监听端口<input name="server_port" type="number" min="1" max="65535" value="8080"></label>
          </div>
        </fieldset>
        <fieldset>
          <legend>数据库</legend>
          <label>连接串<input name="database_url" value="sqlite://data/guglerag.db"></label>
          <div class="hint">可填写 mysql://user:pass@host:3306/db 或 postgresql://user:pass@host:5432/db。</div>
        </fieldset>
        <fieldset>
          <legend>安全与检索</legend>
          <label>JWT 密钥<input name="jwt_secret" value="" placeholder="至少 32 个字符"></label>
          <div class="grid">
            <label>嵌入提供方
              <select name="embedding_provider">
                <option value="stub">暂不启用</option>
                <option value="local">本地模型</option>
                <option value="siliconflow">SiliconFlow</option>
              </select>
            </label>
            <label>嵌入模型<input name="embedding_model" value="BAAI/bge-m3"></label>
          </div>
          <div class="grid">
            <label>SiliconFlow URL<input name="siliconflow_url" value="https://api.siliconflow.cn"></label>
            <label>SiliconFlow API Key<input name="siliconflow_api_key" type="password"></label>
          </div>
        </fieldset>
        <fieldset>
          <legend>MCP</legend>
          <label class="check"><input name="mcp_enabled" type="checkbox" checked>启用 MCP 端点</label>
          <label class="check"><input name="mcp_auth_required" type="checkbox">MCP 调用需要 Bearer Token</label>
        </fieldset>
        <div class="actions">
          <div id="message" class="message"></div>
          <button type="submit">保存 .env</button>
        </div>
      </form>
    </main>
  </div>
  <script>
    const form = document.querySelector("#setup");
    const message = document.querySelector("#message");
    form.jwt_secret.value = crypto.randomUUID().replaceAll("-", "") + crypto.randomUUID().replaceAll("-", "");

    form.addEventListener("submit", async event => {
      event.preventDefault();
      message.className = "message";
      message.textContent = "正在保存...";
      const data = Object.fromEntries(new FormData(form).entries());
      data.server_port = Number(data.server_port);
      data.mcp_enabled = form.mcp_enabled.checked;
      data.mcp_auth_required = form.mcp_auth_required.checked;
      const response = await fetch("/api/setup", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data)
      });
      const body = await response.json().catch(() => ({}));
      if (!response.ok) {
        message.className = "message error";
        message.textContent = body.error || "保存失败";
        return;
      }
      message.className = "message ok";
      message.textContent = ".env 已写入，重启服务后生效。";
    });
  </script>
</body>
</html>"###;

const INDEX_HTML: &str = r###"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>GugleRAG</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f6f7f9;
      --panel: #ffffff;
      --ink: #1d252d;
      --muted: #637083;
      --line: #d9dee7;
      --accent: #0f766e;
      --accent-strong: #0b5f59;
      --danger: #b42318;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--ink); }
    button, input, textarea { font: inherit; }
    button {
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--panel);
      color: var(--ink);
      padding: 8px 11px;
      cursor: pointer;
    }
    button.primary { background: var(--accent); color: white; border-color: var(--accent); }
    button.primary:hover { background: var(--accent-strong); }
    button.danger { color: var(--danger); }
    input, textarea {
      width: 100%;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: white;
      color: var(--ink);
      padding: 9px 10px;
      outline: none;
    }
    textarea { min-height: 460px; resize: vertical; line-height: 1.5; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; }
    .shell { min-height: 100vh; display: grid; grid-template-columns: 280px minmax(0, 1fr) 420px; }
    aside, main, section { min-width: 0; }
    aside { border-right: 1px solid var(--line); background: var(--panel); padding: 18px; }
    main { padding: 18px; }
    section { border-left: 1px solid var(--line); background: var(--panel); padding: 18px; }
    .brand { font-weight: 750; font-size: 20px; margin-bottom: 18px; }
    .muted { color: var(--muted); font-size: 13px; }
    .stack { display: grid; gap: 10px; }
    .row { display: flex; gap: 8px; align-items: center; }
    .row > * { min-width: 0; }
    .toolbar { display: flex; gap: 8px; align-items: center; justify-content: space-between; margin-bottom: 12px; }
    .doc-list { display: grid; gap: 6px; margin-top: 14px; }
    .doc {
      width: 100%;
      text-align: left;
      display: grid;
      gap: 3px;
      border-radius: 6px;
    }
    .doc.active { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
    .doc-title { font-weight: 650; overflow-wrap: anywhere; }
    .editor-head { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 10px; margin-bottom: 10px; }
    .preview {
      background: white;
      border: 1px solid var(--line);
      border-radius: 6px;
      min-height: 460px;
      padding: 16px;
      line-height: 1.55;
      overflow-wrap: anywhere;
    }
    .preview pre { overflow: auto; background: #eef1f5; padding: 12px; border-radius: 6px; }
    .tabs { display: inline-flex; border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
    .tabs button { border: 0; border-radius: 0; }
    .tabs button.active { background: #d9f3ef; color: #074f49; }
    .auth {
      max-width: 380px;
      margin: 12vh auto;
      background: white;
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 22px;
    }
    .results { display: grid; gap: 8px; margin-top: 10px; }
    .result { text-align: left; display: grid; gap: 5px; }
    .error { color: var(--danger); font-size: 13px; min-height: 18px; }
    @media (max-width: 1080px) {
      .shell { grid-template-columns: 240px minmax(0, 1fr); }
      section { grid-column: 1 / -1; border-left: 0; border-top: 1px solid var(--line); }
    }
    @media (max-width: 760px) {
      .shell { display: block; }
      aside, main, section { border: 0; border-bottom: 1px solid var(--line); }
      .editor-head { grid-template-columns: 1fr; }
    }
  </style>
</head>
<body>
  <div id="app"></div>
  <script>
    const state = {
      token: localStorage.getItem("guglerag.token") || "",
      user: null,
      docs: [],
      active: null,
      mode: "edit",
      error: ""
    };

    const app = document.querySelector("#app");

    function authHeaders() {
      return state.token ? { Authorization: `Bearer ${state.token}` } : {};
    }

    async function api(path, options = {}) {
      const res = await fetch(path, {
        ...options,
        headers: {
          "Content-Type": "application/json",
          ...authHeaders(),
          ...(options.headers || {})
        }
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body.error || `${res.status} ${res.statusText}`);
      }
      if (res.status === 204) return null;
      return res.json();
    }

    function render() {
      app.innerHTML = state.token ? layout() : authView();
      bind();
    }

    function authView() {
      return `<div class="auth stack">
        <div>
          <div class="brand">GugleRAG</div>
          <div class="muted">团队 Markdown 知识库</div>
        </div>
        <input id="username" placeholder="用户名">
        <input id="password" type="password" placeholder="密码，至少 8 位">
        <input id="displayName" placeholder="显示名称">
        <div class="row">
          <button class="primary" id="login">登录</button>
          <button id="register">注册</button>
        </div>
        <div class="error">${escapeHtml(state.error)}</div>
      </div>`;
    }

    function layout() {
      const active = state.active || {};
      return `<div class="shell">
        <aside>
          <div class="brand">GugleRAG</div>
          <div class="stack">
            <input id="search" placeholder="搜索文档">
            <button class="primary" id="newDoc">新建文档</button>
            <div class="muted">${state.user ? escapeHtml(state.user.display_name) : ""}</div>
          </div>
          <div class="doc-list">
            ${state.docs.map(docButton).join("")}
          </div>
        </aside>
        <main>
          <div class="toolbar">
            <div class="tabs">
              <button data-mode="edit" class="${state.mode === "edit" ? "active" : ""}">编辑</button>
              <button data-mode="preview" class="${state.mode === "preview" ? "active" : ""}">预览</button>
            </div>
            <div class="row">
              <button class="primary" id="saveDoc" ${active.id ? "" : "disabled"}>保存</button>
              <button class="danger" id="deleteDoc" ${active.id ? "" : "disabled"}>删除</button>
            </div>
          </div>
          <div class="editor-head">
            <input id="title" placeholder="文档标题" value="${escapeAttr(active.title || "")}">
            <input id="tags" placeholder="标签，用逗号分隔" value="${escapeAttr((active.tags || []).join(", "))}">
          </div>
          ${state.mode === "edit"
            ? `<textarea id="content" placeholder="Markdown 内容">${escapeHtml(active.content || "")}</textarea>`
            : `<div class="preview">${markdown(active.content || "")}</div>`}
          <div class="error">${escapeHtml(state.error)}</div>
        </main>
        <section>
          <div class="brand">MCP 工具</div>
          <div class="muted">HTTP JSON-RPC 端点：/mcp</div>
          <pre class="preview" style="min-height:auto; white-space:pre-wrap;">${escapeHtml(mcpExample())}</pre>
          <button id="logout">退出登录</button>
        </section>
      </div>`;
    }

    function docButton(doc) {
      return `<button class="doc ${state.active && state.active.id === doc.id ? "active" : ""}" data-doc="${doc.id}">
        <span class="doc-title">${escapeHtml(doc.title)}</span>
        <span class="muted">${new Date(doc.updated_at).toLocaleString()}</span>
      </button>`;
    }

    function bind() {
      document.querySelector("#login")?.addEventListener("click", () => auth("login"));
      document.querySelector("#register")?.addEventListener("click", () => auth("register"));
      document.querySelector("#newDoc")?.addEventListener("click", newDoc);
      document.querySelector("#saveDoc")?.addEventListener("click", saveDoc);
      document.querySelector("#deleteDoc")?.addEventListener("click", deleteDoc);
      document.querySelector("#logout")?.addEventListener("click", logout);
      document.querySelector("#search")?.addEventListener("input", event => search(event.target.value));
      document.querySelectorAll("[data-doc]").forEach(button => button.addEventListener("click", () => openDoc(button.dataset.doc)));
      document.querySelectorAll("[data-mode]").forEach(button => button.addEventListener("click", () => {
        syncActiveFromForm();
        state.mode = button.dataset.mode;
        render();
      }));
    }

    async function auth(kind) {
      try {
        state.error = "";
        const body = {
          username: document.querySelector("#username").value,
          password: document.querySelector("#password").value,
          display_name: document.querySelector("#displayName").value
        };
        const res = await api(`/api/auth/${kind}`, { method: "POST", body: JSON.stringify(body) });
        state.token = res.token;
        state.user = res.user;
        localStorage.setItem("guglerag.token", state.token);
        await loadDocs();
      } catch (error) {
        state.error = error.message;
        render();
      }
    }

    async function loadMe() {
      if (!state.token) return;
      try {
        state.user = await api("/api/me");
        await loadDocs();
      } catch {
        logout();
      }
    }

    async function loadDocs() {
      state.docs = await api("/api/documents");
      if (!state.active && state.docs[0]) {
        state.active = await api(`/api/documents/${state.docs[0].id}`);
      }
      render();
    }

    async function openDoc(id) {
      state.error = "";
      state.active = await api(`/api/documents/${id}`);
      render();
    }

    async function newDoc() {
      try {
        state.error = "";
        const doc = await api("/api/documents", {
          method: "POST",
          body: JSON.stringify({ title: "未命名文档", content: "# 未命名文档\n", tags: [] })
        });
        state.docs.push(doc);
        state.active = doc;
        render();
      } catch (error) {
        state.error = error.message;
        render();
      }
    }

    async function saveDoc() {
      if (!state.active) return;
      try {
        state.error = "";
        syncActiveFromForm();
        const saved = await api(`/api/documents/${state.active.id}`, {
          method: "PUT",
          body: JSON.stringify({
            title: state.active.title,
            content: state.active.content,
            tags: state.active.tags
          })
        });
        state.active = saved;
        await loadDocs();
      } catch (error) {
        state.error = error.message;
        render();
      }
    }

    async function deleteDoc() {
      if (!state.active || !confirm("删除当前文档？")) return;
      await api(`/api/documents/${state.active.id}`, { method: "DELETE" });
      state.active = null;
      await loadDocs();
    }

    async function search(q) {
      if (!q.trim()) {
        await loadDocs();
        return;
      }
      const results = await api(`/api/search?q=${encodeURIComponent(q)}&limit=20`);
      state.docs = results.map(item => ({ ...item, content: "", tags: [] }));
      render();
      document.querySelector("#search").value = q;
    }

    function syncActiveFromForm() {
      if (!state.active) return;
      const title = document.querySelector("#title");
      const content = document.querySelector("#content");
      const tags = document.querySelector("#tags");
      if (title) state.active.title = title.value;
      if (content) state.active.content = content.value;
      if (tags) state.active.tags = tags.value.split(",").map(tag => tag.trim()).filter(Boolean);
    }

    function logout() {
      state.token = "";
      state.user = null;
      state.docs = [];
      state.active = null;
      localStorage.removeItem("guglerag.token");
      render();
    }

    function markdown(input) {
      return escapeHtml(input)
        .replace(/^### (.*)$/gm, "<h3>$1</h3>")
        .replace(/^## (.*)$/gm, "<h2>$1</h2>")
        .replace(/^# (.*)$/gm, "<h1>$1</h1>")
        .replace(/\*\*(.*?)\*\*/g, "<strong>$1</strong>")
        .replace(/`([^`]+)`/g, "<code>$1</code>")
        .replace(/\n/g, "<br>");
    }

    function mcpExample() {
      return JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "tools/call",
        params: { name: "search_knowledge", arguments: { query: "部署", limit: 5 } }
      }, null, 2);
    }

    function escapeHtml(value) {
      return String(value).replace(/[&<>"']/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" }[ch]));
    }

    function escapeAttr(value) {
      return escapeHtml(value).replace(/`/g, "&#96;");
    }

    loadMe();
    render();
  </script>
</body>
</html>"###;
