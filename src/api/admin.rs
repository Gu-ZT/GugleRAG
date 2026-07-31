use crate::{
    AppState, auth,
    config::{self, Config, SetupRequest},
    domain::{Role, User, Workspace},
    error::AppError,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminConfigRequest {
    server_host: String,
    server_port: u16,
    database_url: String,
    #[serde(default)]
    jwt_secret: Option<String>,
    #[serde(default)]
    registration_enabled: Option<bool>,
    embedding_provider: String,
    embedding_model: String,
    #[serde(default)]
    embedding_url: String,
    #[serde(default)]
    vector_index_path: Option<String>,
    siliconflow_url: String,
    #[serde(default)]
    siliconflow_api_key: Option<String>,
    reranker_enabled: bool,
    reranker_provider: String,
    reranker_model: String,
    reranker_url: String,
    mcp_enabled: bool,
    mcp_auth_required: bool,
    mcp_public_url: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminCreateUserRequest {
    username: String,
    password: String,
    display_name: Option<String>,
    role: Role,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUpdateUserRequest {
    username: String,
    display_name: Option<String>,
    #[serde(default)]
    password: Option<String>,
    role: Role,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminUserResponse {
    id: Uuid,
    username: String,
    display_name: String,
    role: Role,
    created_at: DateTime<Utc>,
    workspaces: Vec<Workspace>,
}

pub(crate) async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    require_admin(&headers, &state).await?;
    let saved = saved_config(&state);
    Ok(Json(config_response(&state, &saved)))
}

pub(crate) async fn save_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdminConfigRequest>,
) -> Result<Json<Value>, AppError> {
    require_admin(&headers, &state).await?;
    let current = saved_config(&state);
    let jwt_secret = replacement_secret(input.jwt_secret, &current.jwt_secret);
    let siliconflow_api_key =
        replacement_secret(input.siliconflow_api_key, &current.siliconflow_api_key);
    let embedding_url = if input.embedding_url.trim().is_empty() {
        current.embedding_url.clone()
    } else {
        input.embedding_url
    };
    let vector_index_path = input
        .vector_index_path
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| current.vector_index_path.to_string_lossy().into_owned());
    let setup = SetupRequest {
        server_host: input.server_host,
        server_port: input.server_port,
        database_url: input.database_url,
        jwt_secret,
        registration_enabled: input
            .registration_enabled
            .unwrap_or(current.registration_enabled),
        embedding_provider: input.embedding_provider,
        embedding_model: input.embedding_model,
        embedding_url,
        vector_index_path,
        siliconflow_url: Some(input.siliconflow_url),
        siliconflow_api_key: Some(siliconflow_api_key),
        reranker_enabled: input.reranker_enabled,
        reranker_provider: input.reranker_provider,
        reranker_model: input.reranker_model,
        reranker_url: input.reranker_url,
        mcp_enabled: input.mcp_enabled,
        mcp_auth_required: input.mcp_auth_required,
        mcp_public_url: Some(input.mcp_public_url),
    };
    config::validate_setup(&setup)?;
    fs::write(&state.config.env_path, config::render_env_file(&setup)).await?;
    let saved = Config::from_file(&state.config.env_path);
    Ok(Json(json!({
        "ok": true,
        "env_path": state.config.env_path,
        "restart_required": !state.config.same_runtime_settings(&saved)
    })))
}

pub(crate) async fn restart(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin(&headers, &state).await?;
    state.restart_tx.send_replace(true);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "ok": true, "restarting": true })),
    ))
}

pub(crate) async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminUserResponse>>, AppError> {
    require_admin(&headers, &state).await?;
    Ok(Json(admin_user_list(&state).await?))
}

pub(crate) async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdminCreateUserRequest>,
) -> Result<(StatusCode, Json<AdminUserResponse>), AppError> {
    require_admin(&headers, &state).await?;
    auth::validate_username(&input.username)?;
    auth::validate_password(&input.password)?;
    ensure_username_available(&state, input.username.trim(), None).await?;
    let username = input.username.trim().to_string();
    let salt = Uuid::new_v4().to_string();
    let user = User {
        id: Uuid::new_v4(),
        username: username.clone(),
        display_name: display_name_or_username(input.display_name, &username),
        password_hash: auth::hash_password(&salt, &input.password),
        salt,
        role: input.role,
        created_at: Utc::now(),
    };
    state.database.insert_user(&user).await?;
    Ok((
        StatusCode::CREATED,
        Json(admin_user_response(&state, user).await?),
    ))
}

pub(crate) async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(input): Json<AdminUpdateUserRequest>,
) -> Result<Json<AdminUserResponse>, AppError> {
    let admin = require_admin(&headers, &state).await?;
    auth::validate_username(&input.username)?;
    ensure_username_available(&state, input.username.trim(), Some(user_id)).await?;
    let mut user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    if user.id == admin.id && input.role != Role::Admin {
        return Err(AppError::Forbidden(
            "cannot remove administrator role from your own account".to_string(),
        ));
    }
    if user.role == Role::Admin
        && input.role != Role::Admin
        && state.database.admin_count().await? <= 1
    {
        return Err(AppError::Conflict(
            "at least one administrator is required".to_string(),
        ));
    }
    let username = input.username.trim().to_string();
    user.username = username.clone();
    user.display_name = display_name_or_username(input.display_name, &username);
    user.role = input.role;
    if let Some(password) = input.password.filter(|value| !value.is_empty()) {
        auth::validate_password(&password)?;
        user.salt = Uuid::new_v4().to_string();
        user.password_hash = auth::hash_password(&user.salt, &password);
    }
    state.database.update_user(&user).await?;
    Ok(Json(admin_user_response(&state, user).await?))
}

pub(crate) async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let admin = require_admin(&headers, &state).await?;
    if admin.id == user_id {
        return Err(AppError::Forbidden(
            "cannot delete your own account".to_string(),
        ));
    }
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    if user.role == Role::Admin && state.database.admin_count().await? <= 1 {
        return Err(AppError::Conflict(
            "at least one administrator is required".to_string(),
        ));
    }
    state.database.delete_user(user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn require_admin(headers: &HeaderMap, state: &AppState) -> Result<User, AppError> {
    let user_id = auth::require_user(headers, state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if user.role != Role::Admin {
        return Err(AppError::Forbidden(
            "administrator access required".to_string(),
        ));
    }
    Ok(user)
}

fn saved_config(state: &AppState) -> Config {
    if state.config.env_path.exists() {
        Config::from_file(&state.config.env_path)
    } else {
        (*state.config).clone()
    }
}

fn replacement_secret(candidate: Option<String>, current: &str) -> String {
    candidate
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| current.to_string())
}

fn config_response(state: &AppState, saved: &Config) -> Value {
    json!({
        "env_path": state.config.env_path,
        "restart_required": !state.config.same_runtime_settings(saved),
        "secrets": {
            "jwt_secret_configured": !saved.jwt_secret.trim().is_empty(),
            "siliconflow_api_key_configured": !saved.siliconflow_api_key.trim().is_empty()
        },
        "current": {
            "server_host": saved.host,
            "server_port": saved.port,
            "database_url": saved.database.url,
            "registration_enabled": saved.registration_enabled,
            "embedding_provider": saved.embedding_provider,
            "embedding_model": saved.embedding_model,
            "embedding_url": saved.embedding_url,
            "vector_index_path": saved.vector_index_path,
            "siliconflow_url": saved.siliconflow_url,
            "reranker_enabled": saved.reranker_enabled,
            "reranker_provider": saved.reranker_provider,
            "reranker_model": saved.reranker_model,
            "reranker_url": saved.reranker_url,
            "mcp_enabled": saved.mcp_enabled,
            "mcp_auth_required": saved.mcp_auth_required,
            "mcp_public_url": saved.mcp_public_url
        }
    })
}

async fn ensure_username_available(
    state: &AppState,
    username: &str,
    existing_user_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(user) = state.database.find_user_by_username(username).await? else {
        return Ok(());
    };
    if Some(user.id) == existing_user_id {
        return Ok(());
    }
    Err(AppError::Conflict("username already exists".to_string()))
}

async fn admin_user_list(state: &AppState) -> Result<Vec<AdminUserResponse>, AppError> {
    let users = state.database.list_users().await?;
    let mut responses = Vec::with_capacity(users.len());
    for user in users {
        responses.push(admin_user_response(state, user).await?);
    }
    Ok(responses)
}

async fn admin_user_response(state: &AppState, user: User) -> Result<AdminUserResponse, AppError> {
    Ok(AdminUserResponse {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        created_at: user.created_at,
        workspaces: state.database.list_workspaces(user.id).await?,
    })
}

fn display_name_or_username(display_name: Option<String>, username: &str) -> String {
    display_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| username.to_string())
}
