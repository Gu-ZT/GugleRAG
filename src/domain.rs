use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Personal,
    Team,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    Owner,
    Admin,
    Member,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub kind: WorkspaceKind,
    pub owner_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamMember {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: TeamRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamInvitation {
    pub id: Uuid,
    pub team_id: Uuid,
    pub team_name: String,
    pub inviter_id: Uuid,
    pub invitee_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeBase {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub scope: String,
    pub team_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub salt: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Editor,
    Reader,
}

pub fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Reader => "reader",
    }
}

pub fn parse_role(value: &str) -> Result<Role, String> {
    match value {
        "admin" => Ok(Role::Admin),
        "editor" => Ok(Role::Editor),
        "reader" => Ok(Role::Reader),
        _ => Err(format!("unknown role: {value}")),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Document {
    pub id: Uuid,
    pub knowledge_base_id: Uuid,
    pub title: String,
    pub content: String,
    pub parent_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub author_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub versions: Vec<DocumentVersion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentVersion {
    pub content: String,
    pub saved_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PublicUser {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
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

#[derive(Debug, Serialize, Clone)]
pub struct SearchResult {
    pub id: Uuid,
    pub title: String,
    pub excerpt: String,
    pub score: usize,
    pub updated_at: DateTime<Utc>,
}
