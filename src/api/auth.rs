use crate::{
    AppState, auth,
    domain::{PublicUser, Role, User},
    error::AppError,
};
use axum::{Json, extract::State, http::HeaderMap};
use chrono::Utc;
use uuid::Uuid;

pub(crate) async fn register(
    State(state): State<AppState>,
    Json(input): Json<auth::RegisterRequest>,
) -> Result<Json<auth::AuthResponse>, AppError> {
    auth::validate_username(&input.username)?;
    auth::validate_password(&input.password)?;
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
    let username = input.username.trim().to_string();
    let user = User {
        id,
        username: username.clone(),
        display_name: input
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(username),
        password_hash: auth::hash_password(&salt, &input.password),
        salt,
        role,
        created_at: Utc::now(),
    };
    state.database.insert_user(&user).await?;
    let token = auth::issue_token(id, &state.config.jwt_secret)?;
    Ok(Json(auth::AuthResponse {
        token,
        user: PublicUser::from(&user),
    }))
}

pub(crate) async fn login(
    State(state): State<AppState>,
    Json(input): Json<auth::LoginRequest>,
) -> Result<Json<auth::AuthResponse>, AppError> {
    let user = state
        .database
        .find_user_by_username(input.username.trim())
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid username or password".to_string()))?;
    if user.password_hash != auth::hash_password(&user.salt, &input.password) {
        return Err(AppError::Unauthorized(
            "invalid username or password".to_string(),
        ));
    }
    let token = auth::issue_token(user.id, &state.config.jwt_secret)?;
    Ok(Json(auth::AuthResponse {
        token,
        user: PublicUser::from(&user),
    }))
}

pub(crate) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicUser>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    Ok(Json(PublicUser::from(&user)))
}
