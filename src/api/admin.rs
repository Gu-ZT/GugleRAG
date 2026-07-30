use crate::{
    AppState, auth,
    config::{self, Config, SetupRequest},
    domain::Role,
    error::AppError,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminConfigRequest {
    server_host: String,
    server_port: u16,
    database_url: String,
    #[serde(default)]
    jwt_secret: Option<String>,
    embedding_provider: String,
    embedding_model: String,
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
    let setup = SetupRequest {
        server_host: input.server_host,
        server_port: input.server_port,
        database_url: input.database_url,
        jwt_secret,
        embedding_provider: input.embedding_provider,
        embedding_model: input.embedding_model,
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

async fn require_admin(headers: &HeaderMap, state: &AppState) -> Result<(), AppError> {
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
    Ok(())
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
            "embedding_provider": saved.embedding_provider,
            "embedding_model": saved.embedding_model,
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
