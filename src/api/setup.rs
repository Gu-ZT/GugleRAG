use crate::{
    AppState,
    config::{self, SetupRequest},
    error::AppError,
};
use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};
use tokio::fs;

pub(crate) async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "GugleRAG",
        "setup_required": state.config.setup_required,
        "database": {
            "engine": &state.config.database.engine,
            "url": state.config.database.redacted_url()
        },
        "mcp_enabled": state.config.mcp_enabled,
        "registration_enabled": state.config.registration_enabled,
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

pub(crate) async fn setup_status(State(state): State<AppState>) -> Json<Value> {
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
            "registration_enabled": state.config.registration_enabled,
            "embedding_provider": state.config.embedding_provider,
            "embedding_model": state.config.embedding_model,
            "reranker_enabled": state.config.reranker_enabled,
            "reranker_provider": state.config.reranker_provider,
            "reranker_model": state.config.reranker_model,
            "reranker_url": state.config.reranker_url
        }
    }))
}

pub(crate) async fn save_setup(
    State(state): State<AppState>,
    Json(input): Json<SetupRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if !state.config.setup_required {
        return Err(AppError::Conflict(
            ".env already exists; edit it directly and restart the server".to_string(),
        ));
    }
    config::validate_setup(&input)?;
    fs::write(&state.config.env_path, config::render_env_file(&input)).await?;
    state.restart_tx.send_replace(true);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "ok": true,
            "env_path": state.config.env_path,
            "restart_required": true,
            "restarting": true
        })),
    ))
}
