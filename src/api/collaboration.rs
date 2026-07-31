use crate::{
    AppState, auth,
    db::McpTokenRecord,
    domain::{
        KnowledgeBase, Role, Team, TeamInvitation, TeamMember, TeamRole, Workspace, WorkspaceKind,
    },
    error::AppError,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_MCP_TOKEN_EXPIRY_DAYS: u32 = 30;
const MAX_MCP_TOKEN_EXPIRY_DAYS: u32 = 3650;

#[derive(Debug, Deserialize)]
pub(crate) struct NameRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct KnowledgeBaseRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InvitationRequest {
    username: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct InvitationResponse {
    invitation: TeamInvitation,
    invite_token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpConfigRequest {
    scope: String,
    workspace_id: Option<Uuid>,
    #[serde(default = "default_mcp_token_expiry_days")]
    expires_in_days: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpHeaders {
    #[serde(rename = "Authorization")]
    authorization: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpConfigResponse {
    #[serde(rename = "type")]
    config_type: &'static str,
    url: String,
    headers: McpHeaders,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpTokenResponse {
    id: Uuid,
    token_prefix: String,
    scope: String,
    workspace_id: Option<Uuid>,
    workspace_name: Option<String>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<McpTokenRecord> for McpTokenResponse {
    fn from(token: McpTokenRecord) -> Self {
        Self {
            id: token.id,
            token_prefix: token.token_prefix,
            scope: token.scope,
            workspace_id: token.workspace_id,
            workspace_name: token.workspace_name,
            expires_at: token.expires_at,
            revoked_at: token.revoked_at,
            created_at: token.created_at,
        }
    }
}

fn default_mcp_token_expiry_days() -> u32 {
    DEFAULT_MCP_TOKEN_EXPIRY_DAYS
}

pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Workspace>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    state.database.ensure_personal_workspace(user_id).await?;
    Ok(Json(state.database.list_workspaces(user_id).await?))
}

pub(crate) async fn list_knowledge_bases(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<KnowledgeBase>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    require_workspace_access(&state, user_id, workspace_id).await?;
    Ok(Json(
        state.database.list_knowledge_bases(workspace_id).await?,
    ))
}

pub(crate) async fn create_knowledge_base(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(input): Json<KnowledgeBaseRequest>,
) -> Result<Json<KnowledgeBase>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let workspace = require_workspace_access(&state, user_id, workspace_id).await?;
    if let Some(team_id) = workspace.team_id {
        let role = state.database.team_member_role(team_id, user_id).await?;
        if !matches!(role, Some(TeamRole::Owner | TeamRole::Admin)) {
            return Err(AppError::Forbidden(
                "only team owners and admins can create knowledge bases".to_string(),
            ));
        }
    }
    let knowledge_base = KnowledgeBase {
        id: Uuid::new_v4(),
        workspace_id,
        name: auth::require_non_empty(Some(input.name), "name")?,
        description: input.description.trim().to_string(),
        created_at: Utc::now(),
    };
    state
        .database
        .insert_knowledge_base(&knowledge_base)
        .await?;
    Ok(Json(knowledge_base))
}

pub(crate) async fn delete_knowledge_base(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(knowledge_base_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = require_editor(&state, &headers).await?;
    let knowledge_base = require_knowledge_base_access(&state, user_id, knowledge_base_id).await?;
    let workspace = require_workspace_access(&state, user_id, knowledge_base.workspace_id).await?;
    if let Some(team_id) = workspace.team_id {
        let role = state.database.team_member_role(team_id, user_id).await?;
        if !matches!(role, Some(TeamRole::Owner | TeamRole::Admin)) {
            return Err(AppError::Forbidden(
                "only team owners and admins can delete knowledge bases".to_string(),
            ));
        }
    }
    state
        .search
        .remove_knowledge_base_index(knowledge_base_id)?;
    state
        .database
        .delete_knowledge_base(knowledge_base_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_teams(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Team>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    Ok(Json(state.database.list_teams(user_id).await?))
}

pub(crate) async fn create_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<NameRequest>,
) -> Result<Json<Team>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let name = auth::require_non_empty(Some(input.name), "name")?;
    Ok(Json(state.database.create_team(user_id, &name).await?))
}

pub(crate) async fn list_team_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<Uuid>,
) -> Result<Json<Vec<TeamMember>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    require_team_membership(&state, team_id, user_id).await?;
    Ok(Json(state.database.list_team_members(team_id).await?))
}

pub(crate) async fn create_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<Uuid>,
    Json(input): Json<InvitationRequest>,
) -> Result<Json<InvitationResponse>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let role = require_team_membership(&state, team_id, user_id).await?;
    if !matches!(role, TeamRole::Owner | TeamRole::Admin) {
        return Err(AppError::Forbidden(
            "only team owners and admins can invite members".to_string(),
        ));
    }
    let invitee = state
        .database
        .find_user_by_username(input.username.trim())
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    if state
        .database
        .team_member_role(team_id, invitee.id)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(
            "user is already a team member".to_string(),
        ));
    }
    let invite_token = Uuid::new_v4().to_string();
    let invitation = state
        .database
        .create_team_invitation(
            team_id,
            user_id,
            invitee.id,
            &auth::hash_access_token(&invite_token),
        )
        .await?;
    Ok(Json(InvitationResponse {
        invitation,
        invite_token,
    }))
}

pub(crate) async fn list_invitations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TeamInvitation>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    Ok(Json(state.database.list_invitations(user_id).await?))
}

pub(crate) async fn accept_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Json<Team>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let team = state
        .database
        .accept_invitation(&auth::hash_access_token(&token), user_id)
        .await?;
    Ok(Json(team))
}

pub(crate) async fn create_mcp_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<McpConfigRequest>,
) -> Result<Json<McpConfigResponse>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    if !(1..=MAX_MCP_TOKEN_EXPIRY_DAYS).contains(&input.expires_in_days) {
        return Err(AppError::BadRequest(format!(
            "expires_in_days must be between 1 and {MAX_MCP_TOKEN_EXPIRY_DAYS}"
        )));
    }
    let path = match input.scope.as_str() {
        "all" => {
            if input.workspace_id.is_some() {
                return Err(AppError::BadRequest(
                    "workspace_id is not valid for all scope".to_string(),
                ));
            }
            "mcp/all".to_string()
        }
        "user" | "group" => {
            let workspace_id = input.workspace_id.ok_or_else(|| {
                AppError::BadRequest(format!(
                    "workspace_id is required for {} scope",
                    input.scope
                ))
            })?;
            let workspace = require_workspace_access(&state, user_id, workspace_id).await?;
            let expected_kind = if input.scope == "user" {
                WorkspaceKind::Personal
            } else {
                WorkspaceKind::Team
            };
            if workspace.kind != expected_kind {
                return Err(AppError::BadRequest(format!(
                    "workspace_id is not a {} workspace",
                    input.scope
                )));
            }
            format!("mcp/{}/{workspace_id}", input.scope)
        }
        _ => {
            return Err(AppError::BadRequest(
                "scope must be user, group, or all".to_string(),
            ));
        }
    };
    let token = format!("ggr_{}", Uuid::new_v4().simple());
    let token_prefix = token.chars().take(12).collect::<String>();
    state
        .database
        .create_mcp_token(
            user_id,
            &auth::hash_access_token(&token),
            &token_prefix,
            &input.scope,
            input.workspace_id,
            Utc::now() + Duration::days(i64::from(input.expires_in_days)),
        )
        .await?;
    let base_url = mcp_base_url(&state);
    Ok(Json(McpConfigResponse {
        config_type: "http",
        url: format!("{base_url}/{path}"),
        headers: McpHeaders {
            authorization: format!("Bearer {token}"),
        },
    }))
}

pub(crate) async fn list_mcp_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<McpTokenResponse>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let tokens = state
        .database
        .list_mcp_tokens(user_id)
        .await?
        .into_iter()
        .map(McpTokenResponse::from)
        .collect();
    Ok(Json(tokens))
}

pub(crate) async fn revoke_mcp_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    state.database.revoke_mcp_token(user_id, token_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn require_knowledge_base_access(
    state: &AppState,
    user_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<KnowledgeBase, AppError> {
    let knowledge_base = state
        .database
        .get_knowledge_base(knowledge_base_id)
        .await?
        .ok_or_else(|| AppError::NotFound("knowledge base not found".to_string()))?;
    require_workspace_access(state, user_id, knowledge_base.workspace_id).await?;
    Ok(knowledge_base)
}

pub(crate) async fn default_knowledge_base(
    state: &AppState,
    user_id: Uuid,
) -> Result<KnowledgeBase, AppError> {
    let (_, knowledge_base) = state.database.ensure_personal_workspace(user_id).await?;
    Ok(knowledge_base)
}

async fn require_editor(state: &AppState, headers: &HeaderMap) -> Result<Uuid, AppError> {
    let user_id = auth::require_user(headers, state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !matches!(user.role, Role::Admin | Role::Editor) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    Ok(user_id)
}

async fn require_workspace_access(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Workspace, AppError> {
    if !state
        .database
        .user_can_access_workspace(user_id, workspace_id)
        .await?
    {
        return Err(AppError::Forbidden("workspace access denied".to_string()));
    }
    state
        .database
        .get_workspace(workspace_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".to_string()))
}

async fn require_team_membership(
    state: &AppState,
    team_id: Uuid,
    user_id: Uuid,
) -> Result<TeamRole, AppError> {
    state
        .database
        .team_member_role(team_id, user_id)
        .await?
        .ok_or_else(|| AppError::Forbidden("team membership required".to_string()))
}

fn mcp_base_url(state: &AppState) -> String {
    if !state.config.mcp_public_url.trim().is_empty() {
        return state
            .config
            .mcp_public_url
            .trim_end_matches('/')
            .to_string();
    }
    let host = match state.config.host.as_str() {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        host => host,
    };
    format!("http://{host}:{}", state.config.port)
}
