use crate::{
    domain::{
        Document, DocumentVersion, KnowledgeBase, Team, TeamInvitation, TeamMember, TeamRole, User,
        Workspace, WorkspaceKind, parse_role, role_to_str,
    },
    error::AppError,
};
use chrono::{DateTime, Utc};
use serde_json::from_str;
use sqlx::{AnyPool, Row, any::AnyPoolOptions};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

macro_rules! db_query {
    ($database:expr, $sql:expr $(,)?) => {
        ::sqlx::query($database.sql($sql))
    };
}

static POSTGRES_QUERY_CACHE: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
enum SqlDialect {
    QuestionMark,
    Postgres,
}

#[derive(Clone)]
pub struct Database {
    pub(crate) pool: AnyPool,
    dialect: SqlDialect,
}

#[derive(Clone)]
pub(crate) struct DocumentEmbedding {
    pub(crate) document_id: Uuid,
    pub(crate) knowledge_base_id: Uuid,
    pub(crate) content_hash: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) vector: Vec<f32>,
}

#[derive(Clone)]
pub(crate) struct DocumentEmbeddingChunk {
    pub(crate) document_id: Uuid,
    pub(crate) chunk_index: usize,
    pub(crate) knowledge_base_id: Uuid,
    pub(crate) content_hash: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) vector: Vec<f32>,
}

#[derive(Clone)]
pub(crate) struct McpTokenRecord {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) token_prefix: String,
    pub(crate) scope: String,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) workspace_name: Option<String>,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) revoked_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self, AppError> {
        let dialect = if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            SqlDialect::Postgres
        } else {
            SqlDialect::QuestionMark
        };
        let pool = AnyPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await?;
        Ok(Self { pool, dialect })
    }

    fn sql(&self, sql: &'static str) -> &'static str {
        match self.dialect {
            SqlDialect::QuestionMark => sql,
            SqlDialect::Postgres => postgres_sql(sql),
        }
    }

    pub(crate) async fn migrate(&self) -> Result<(), AppError> {
        db_query!(
            self,
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
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS teams (
                id VARCHAR(36) PRIMARY KEY,
                name VARCHAR(160) NOT NULL,
                owner_id VARCHAR(36) NOT NULL,
                workspace_id VARCHAR(36) NOT NULL,
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS team_members (
                team_id VARCHAR(36) NOT NULL,
                user_id VARCHAR(36) NOT NULL,
                role VARCHAR(16) NOT NULL,
                joined_at VARCHAR(40) NOT NULL,
                PRIMARY KEY (team_id, user_id)
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS workspaces (
                id VARCHAR(36) PRIMARY KEY,
                name VARCHAR(160) NOT NULL,
                kind VARCHAR(16) NOT NULL,
                owner_id VARCHAR(36),
                team_id VARCHAR(36),
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS knowledge_bases (
                id VARCHAR(36) PRIMARY KEY,
                workspace_id VARCHAR(36) NOT NULL,
                name VARCHAR(160) NOT NULL,
                description TEXT NOT NULL,
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS team_invitations (
                id VARCHAR(36) PRIMARY KEY,
                team_id VARCHAR(36) NOT NULL,
                inviter_id VARCHAR(36) NOT NULL,
                invitee_id VARCHAR(36) NOT NULL,
                token_hash VARCHAR(128) NOT NULL UNIQUE,
                status VARCHAR(16) NOT NULL,
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS mcp_tokens (
                id VARCHAR(36) PRIMARY KEY,
                user_id VARCHAR(36) NOT NULL,
                token_hash VARCHAR(128) NOT NULL UNIQUE,
                token_prefix VARCHAR(32) NOT NULL,
                scope VARCHAR(16) NOT NULL,
                workspace_id VARCHAR(36),
                expires_at VARCHAR(40) NOT NULL,
                revoked_at VARCHAR(40),
                created_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS documents (
                id VARCHAR(36) PRIMARY KEY,
                knowledge_base_id VARCHAR(36),
                title VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                parent_id VARCHAR(36),
                is_folder INTEGER NOT NULL DEFAULT 0,
                tags TEXT NOT NULL,
                author_id VARCHAR(36) NOT NULL,
                created_at VARCHAR(40) NOT NULL,
                updated_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS document_versions (
                id VARCHAR(36) PRIMARY KEY,
                document_id VARCHAR(36) NOT NULL,
                content TEXT NOT NULL,
                saved_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        // Keep both SQL vector tables so upgrades remain non-destructive. They are migration
        // sources only; active retrieval uses the embedded HNSW files managed by SearchEngine.
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS document_embeddings (
                document_id VARCHAR(36) PRIMARY KEY,
                knowledge_base_id VARCHAR(36) NOT NULL,
                content_hash VARCHAR(64) NOT NULL,
                provider VARCHAR(32) NOT NULL,
                model VARCHAR(255) NOT NULL,
                dimensions INTEGER NOT NULL,
                embedding TEXT NOT NULL,
                created_at VARCHAR(40) NOT NULL,
                updated_at VARCHAR(40) NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "CREATE TABLE IF NOT EXISTS document_embedding_chunks (
                document_id VARCHAR(36) NOT NULL,
                chunk_index INTEGER NOT NULL,
                knowledge_base_id VARCHAR(36) NOT NULL,
                content_hash VARCHAR(64) NOT NULL,
                provider VARCHAR(32) NOT NULL,
                model VARCHAR(255) NOT NULL,
                dimensions INTEGER NOT NULL,
                embedding TEXT NOT NULL,
                created_at VARCHAR(40) NOT NULL,
                updated_at VARCHAR(40) NOT NULL,
                PRIMARY KEY (document_id, chunk_index)
            )",
        )
        .execute(&self.pool)
        .await?;
        // Existing installations may predate knowledge-base ownership and folders.
        // Ignoring duplicate-column errors keeps these migrations repeatable.
        let _ = db_query!(
            self,
            "ALTER TABLE documents ADD COLUMN knowledge_base_id VARCHAR(36)"
        )
        .execute(&self.pool)
        .await;
        let _ = db_query!(
            self,
            "ALTER TABLE documents ADD COLUMN is_folder INTEGER NOT NULL DEFAULT 0"
        )
        .execute(&self.pool)
        .await;
        self.ensure_personal_workspaces().await?;
        Ok(())
    }

    async fn ensure_personal_workspaces(&self) -> Result<(), AppError> {
        let rows = db_query!(self, "SELECT id FROM app_users")
            .fetch_all(&self.pool)
            .await?;
        for row in rows {
            let user_id = parse_uuid_str(row.try_get("id")?)?;
            self.ensure_personal_workspace(user_id).await?;
        }
        Ok(())
    }

    pub(crate) async fn ensure_personal_workspace(
        &self,
        user_id: Uuid,
    ) -> Result<(Workspace, KnowledgeBase), AppError> {
        let workspace_row = db_query!(
            self,
            "SELECT id, name, kind, owner_id, team_id FROM workspaces
             WHERE kind = 'personal' AND owner_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let workspace = if let Some(row) = workspace_row {
            row_to_workspace(row)?
        } else {
            let workspace = Workspace {
                id: Uuid::new_v4(),
                name: "个人工作区".to_string(),
                kind: WorkspaceKind::Personal,
                owner_id: Some(user_id),
                team_id: None,
            };
            db_query!(
                self,
                "INSERT INTO workspaces (id, name, kind, owner_id, team_id, created_at)
                 VALUES (?, ?, 'personal', ?, NULL, ?)",
            )
            .bind(workspace.id.to_string())
            .bind(&workspace.name)
            .bind(user_id.to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
            workspace
        };
        let kb_row = db_query!(
            self,
            "SELECT id, workspace_id, name, description, created_at
             FROM knowledge_bases WHERE workspace_id = ? ORDER BY created_at LIMIT 1",
        )
        .bind(workspace.id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let knowledge_base = if let Some(row) = kb_row {
            row_to_knowledge_base(row)?
        } else {
            let knowledge_base = KnowledgeBase {
                id: Uuid::new_v4(),
                workspace_id: workspace.id,
                name: "默认知识库".to_string(),
                description: "个人工作区默认知识库".to_string(),
                created_at: Utc::now(),
            };
            self.insert_knowledge_base(&knowledge_base).await?;
            knowledge_base
        };
        db_query!(
            self,
            "UPDATE documents SET knowledge_base_id = ?
             WHERE author_id = ? AND knowledge_base_id IS NULL",
        )
        .bind(knowledge_base.id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok((workspace, knowledge_base))
    }

    pub(crate) async fn user_count(&self) -> Result<i64, AppError> {
        let row = db_query!(self, "SELECT COUNT(*) AS count FROM app_users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<i64, _>("count")?)
    }

    pub(crate) async fn find_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>, AppError> {
        let row = db_query!(
            self,
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    pub(crate) async fn get_user(&self, id: Uuid) -> Result<Option<User>, AppError> {
        let row = db_query!(
            self,
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    pub(crate) async fn list_users(&self) -> Result<Vec<User>, AppError> {
        let rows = db_query!(
            self,
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users ORDER BY LOWER(username)",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_user).collect()
    }

    pub(crate) async fn admin_count(&self) -> Result<i64, AppError> {
        let row = db_query!(
            self,
            "SELECT COUNT(*) AS count FROM app_users WHERE role = 'admin'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("count")?)
    }

    pub(crate) async fn insert_user(&self, user: &User) -> Result<(), AppError> {
        db_query!(
            self,
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
        self.ensure_personal_workspace(user.id).await?;
        Ok(())
    }

    pub(crate) async fn update_user(&self, user: &User) -> Result<(), AppError> {
        db_query!(
            self,
            "UPDATE app_users
             SET username = ?, display_name = ?, password_hash = ?, salt = ?, role = ?
             WHERE id = ?",
        )
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(&user.salt)
        .bind(role_to_str(user.role))
        .bind(user.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_user(&self, user_id: Uuid) -> Result<(), AppError> {
        if self.get_user(user_id).await?.is_none() {
            return Err(AppError::NotFound("user not found".to_string()));
        }
        let personal_workspace_rows = db_query!(
            self,
            "SELECT id FROM workspaces WHERE kind = 'personal' AND owner_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut workspace_ids = personal_workspace_rows
            .into_iter()
            .map(|row| parse_uuid_str(row.try_get::<String, _>("id")?))
            .collect::<Result<Vec<_>, _>>()?;
        let owned_team_rows = db_query!(
            self,
            "SELECT id, workspace_id FROM teams WHERE owner_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut owned_team_ids = Vec::with_capacity(owned_team_rows.len());
        for row in owned_team_rows {
            owned_team_ids.push(parse_uuid_str(row.try_get::<String, _>("id")?)?);
            workspace_ids.push(parse_uuid_str(row.try_get::<String, _>("workspace_id")?)?);
        }
        for workspace_id in &workspace_ids {
            self.delete_workspace_content(*workspace_id).await?;
        }
        for team_id in owned_team_ids {
            db_query!(self, "DELETE FROM team_invitations WHERE team_id = ?")
                .bind(team_id.to_string())
                .execute(&self.pool)
                .await?;
            db_query!(self, "DELETE FROM team_members WHERE team_id = ?")
                .bind(team_id.to_string())
                .execute(&self.pool)
                .await?;
            db_query!(self, "DELETE FROM teams WHERE id = ?")
                .bind(team_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        for workspace_id in workspace_ids {
            db_query!(self, "DELETE FROM workspaces WHERE id = ?")
                .bind(workspace_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        db_query!(
            self,
            "DELETE FROM team_invitations WHERE inviter_id = ? OR invitee_id = ?",
        )
        .bind(user_id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await?;
        db_query!(self, "DELETE FROM team_members WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await?;
        db_query!(self, "DELETE FROM mcp_tokens WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await?;
        db_query!(self, "DELETE FROM app_users WHERE id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn create_mcp_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        token_prefix: &str,
        scope: &str,
        workspace_id: Option<Uuid>,
        expires_at: DateTime<Utc>,
    ) -> Result<McpTokenRecord, AppError> {
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        db_query!(
            self,
            "INSERT INTO mcp_tokens
             (id, user_id, token_hash, token_prefix, scope, workspace_id, expires_at, revoked_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?)",
        )
            .bind(id.to_string())
            .bind(user_id.to_string())
            .bind(token_hash)
            .bind(token_prefix)
            .bind(scope)
            .bind(workspace_id.map(|value| value.to_string()))
            .bind(expires_at.to_rfc3339())
            .bind(created_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        let workspace_name = match workspace_id {
            Some(workspace_id) => self
                .get_workspace(workspace_id)
                .await?
                .map(|workspace| workspace.name),
            None => None,
        };
        Ok(McpTokenRecord {
            id,
            user_id,
            token_prefix: token_prefix.to_string(),
            scope: scope.to_string(),
            workspace_id,
            workspace_name,
            expires_at,
            revoked_at: None,
            created_at,
        })
    }

    pub(crate) async fn list_mcp_tokens(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<McpTokenRecord>, AppError> {
        let rows = db_query!(
            self,
            "SELECT t.id, t.user_id, t.token_prefix, t.scope, t.workspace_id,
                    w.name AS workspace_name, t.expires_at, t.revoked_at, t.created_at
             FROM mcp_tokens t LEFT JOIN workspaces w ON w.id = t.workspace_id
             WHERE t.user_id = ? ORDER BY t.created_at DESC",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_mcp_token).collect()
    }

    pub(crate) async fn find_mcp_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<McpTokenRecord>, AppError> {
        db_query!(
            self,
            "SELECT t.id, t.user_id, t.token_prefix, t.scope, t.workspace_id,
                    w.name AS workspace_name, t.expires_at, t.revoked_at, t.created_at
             FROM mcp_tokens t LEFT JOIN workspaces w ON w.id = t.workspace_id
             WHERE t.token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?
        .map(row_to_mcp_token)
        .transpose()
    }

    pub(crate) async fn revoke_mcp_token(
        &self,
        user_id: Uuid,
        token_id: Uuid,
    ) -> Result<(), AppError> {
        let token = self
            .list_mcp_tokens(user_id)
            .await?
            .into_iter()
            .find(|token| token.id == token_id)
            .ok_or_else(|| AppError::NotFound("MCP token not found".to_string()))?;
        if token.revoked_at.is_some() {
            return Ok(());
        }
        db_query!(
            self,
            "UPDATE mcp_tokens SET revoked_at = ? WHERE id = ? AND user_id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(token_id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_workspaces(&self, user_id: Uuid) -> Result<Vec<Workspace>, AppError> {
        let mut workspaces = db_query!(
            self,
            "SELECT id, name, kind, owner_id, team_id FROM workspaces
             WHERE kind = 'personal' AND owner_id = ? ORDER BY name",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(row_to_workspace)
        .collect::<Result<Vec<_>, _>>()?;
        let team_rows = db_query!(
            self,
            "SELECT w.id, w.name, w.kind, w.owner_id, w.team_id
             FROM workspaces w JOIN team_members m ON m.team_id = w.team_id
             WHERE w.kind = 'team' AND m.user_id = ? ORDER BY w.name",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        workspaces.extend(
            team_rows
                .into_iter()
                .map(row_to_workspace)
                .collect::<Result<Vec<_>, _>>()?,
        );
        Ok(workspaces)
    }

    pub(crate) async fn all_workspaces(&self) -> Result<Vec<Workspace>, AppError> {
        let rows = db_query!(
            self,
            "SELECT id, name, kind, owner_id, team_id FROM workspaces ORDER BY LOWER(name)",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_workspace).collect()
    }

    pub(crate) async fn get_workspace(&self, id: Uuid) -> Result<Option<Workspace>, AppError> {
        db_query!(
            self,
            "SELECT id, name, kind, owner_id, team_id FROM workspaces WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(row_to_workspace)
        .transpose()
    }

    pub(crate) async fn user_can_access_workspace(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, AppError> {
        let row = db_query!(
            self,
            "SELECT w.kind, w.owner_id, w.team_id, m.user_id AS member_user_id
             FROM workspaces w LEFT JOIN team_members m
             ON m.team_id = w.team_id AND m.user_id = ? WHERE w.id = ?",
        )
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(false) };
        let kind: String = row.try_get("kind")?;
        let owner_id: Option<String> = row.try_get("owner_id")?;
        let member_user_id: Option<String> = row.try_get("member_user_id")?;
        Ok(
            (kind == "personal" && owner_id.as_deref() == Some(&user_id.to_string()))
                || member_user_id.is_some(),
        )
    }

    pub(crate) async fn list_knowledge_bases(
        &self,
        workspace_id: Uuid,
    ) -> Result<Vec<KnowledgeBase>, AppError> {
        let rows = db_query!(
            self,
            "SELECT id, workspace_id, name, description, created_at
             FROM knowledge_bases WHERE workspace_id = ? ORDER BY LOWER(name)",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_knowledge_base).collect()
    }

    pub(crate) async fn get_knowledge_base(
        &self,
        id: Uuid,
    ) -> Result<Option<KnowledgeBase>, AppError> {
        db_query!(
            self,
            "SELECT id, workspace_id, name, description, created_at
             FROM knowledge_bases WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(row_to_knowledge_base)
        .transpose()
    }

    pub(crate) async fn insert_knowledge_base(
        &self,
        knowledge_base: &KnowledgeBase,
    ) -> Result<(), AppError> {
        db_query!(
            self,
            "INSERT INTO knowledge_bases (id, workspace_id, name, description, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(knowledge_base.id.to_string())
        .bind(knowledge_base.workspace_id.to_string())
        .bind(&knowledge_base.name)
        .bind(&knowledge_base.description)
        .bind(knowledge_base.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_knowledge_base(&self, id: Uuid) -> Result<(), AppError> {
        if self.get_knowledge_base(id).await?.is_none() {
            return Err(AppError::NotFound("knowledge base not found".to_string()));
        }
        db_query!(
            self,
            "DELETE FROM document_versions
             WHERE document_id IN (SELECT id FROM documents WHERE knowledge_base_id = ?)",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "DELETE FROM document_embedding_chunks WHERE knowledge_base_id = ?"
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "DELETE FROM document_embeddings WHERE knowledge_base_id = ?"
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        db_query!(self, "DELETE FROM documents WHERE knowledge_base_id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        db_query!(self, "DELETE FROM knowledge_bases WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_workspace_content(&self, workspace_id: Uuid) -> Result<(), AppError> {
        let rows = db_query!(
            self,
            "SELECT id FROM knowledge_bases WHERE workspace_id = ?",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let knowledge_base_ids = rows
            .into_iter()
            .map(|row| parse_uuid_str(row.try_get::<String, _>("id")?))
            .collect::<Result<Vec<_>, _>>()?;
        for knowledge_base_id in knowledge_base_ids {
            self.delete_knowledge_base(knowledge_base_id).await?;
        }
        Ok(())
    }

    pub(crate) async fn accessible_knowledge_bases(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<KnowledgeBase>, AppError> {
        let rows = db_query!(
            self,
            "SELECT kb.id, kb.workspace_id, kb.name, kb.description, kb.created_at
             FROM knowledge_bases kb JOIN workspaces w ON w.id = kb.workspace_id
             WHERE (w.kind = 'personal' AND w.owner_id = ?)
                OR EXISTS (
                    SELECT 1 FROM team_members m
                    WHERE m.team_id = w.team_id AND m.user_id = ?
                )
             ORDER BY LOWER(kb.name)",
        )
        .bind(user_id.to_string())
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_knowledge_base).collect()
    }

    pub(crate) async fn create_team(&self, owner_id: Uuid, name: &str) -> Result<Team, AppError> {
        let team_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        db_query!(
            self,
            "INSERT INTO workspaces (id, name, kind, owner_id, team_id, created_at)
             VALUES (?, ?, 'team', NULL, ?, ?)",
        )
        .bind(workspace_id.to_string())
        .bind(name)
        .bind(team_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "INSERT INTO teams (id, name, owner_id, workspace_id, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(team_id.to_string())
        .bind(name)
        .bind(owner_id.to_string())
        .bind(workspace_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "INSERT INTO team_members (team_id, user_id, role, joined_at)
             VALUES (?, ?, 'owner', ?)",
        )
        .bind(team_id.to_string())
        .bind(owner_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        let knowledge_base = KnowledgeBase {
            id: Uuid::new_v4(),
            workspace_id,
            name: "默认知识库".to_string(),
            description: "团队工作区默认知识库".to_string(),
            created_at: now,
        };
        self.insert_knowledge_base(&knowledge_base).await?;
        Ok(Team {
            id: team_id,
            name: name.to_string(),
            owner_id,
            workspace_id,
            created_at: now,
        })
    }

    pub(crate) async fn list_teams(&self, user_id: Uuid) -> Result<Vec<Team>, AppError> {
        let rows = db_query!(
            self,
            "SELECT t.id, t.name, t.owner_id, t.workspace_id, t.created_at
             FROM teams t JOIN team_members m ON m.team_id = t.id
             WHERE m.user_id = ? ORDER BY LOWER(t.name)",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_team).collect()
    }

    pub(crate) async fn get_team(&self, team_id: Uuid) -> Result<Option<Team>, AppError> {
        db_query!(
            self,
            "SELECT id, name, owner_id, workspace_id, created_at FROM teams WHERE id = ?"
        )
        .bind(team_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(row_to_team)
        .transpose()
    }

    pub(crate) async fn team_member_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<TeamRole>, AppError> {
        let row = db_query!(
            self,
            "SELECT role FROM team_members WHERE team_id = ? AND user_id = ?"
        )
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| parse_team_role(row.try_get::<String, _>("role")?))
            .transpose()
    }

    pub(crate) async fn list_team_members(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMember>, AppError> {
        let rows = db_query!(
            self,
            "SELECT m.user_id, u.username, u.display_name, m.role, m.joined_at
             FROM team_members m JOIN app_users u ON u.id = m.user_id
             WHERE m.team_id = ? ORDER BY m.joined_at",
        )
        .bind(team_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_team_member).collect()
    }

    pub(crate) async fn create_team_invitation(
        &self,
        team_id: Uuid,
        inviter_id: Uuid,
        invitee_id: Uuid,
        token_hash: &str,
    ) -> Result<TeamInvitation, AppError> {
        let team = self
            .get_team(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("team not found".to_string()))?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        db_query!(
            self,
            "INSERT INTO team_invitations
             (id, team_id, inviter_id, invitee_id, token_hash, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?)",
        )
        .bind(id.to_string())
        .bind(team_id.to_string())
        .bind(inviter_id.to_string())
        .bind(invitee_id.to_string())
        .bind(token_hash)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(TeamInvitation {
            id,
            team_id,
            team_name: team.name,
            inviter_id,
            invitee_id,
            status: "pending".to_string(),
            created_at: now,
        })
    }

    pub(crate) async fn list_invitations(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<TeamInvitation>, AppError> {
        let rows = db_query!(
            self,
            "SELECT i.id, i.team_id, t.name AS team_name, i.inviter_id,
                    i.invitee_id, i.status, i.created_at
             FROM team_invitations i JOIN teams t ON t.id = i.team_id
             WHERE i.invitee_id = ? ORDER BY i.created_at DESC",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_invitation).collect()
    }

    pub(crate) async fn accept_invitation(
        &self,
        token_hash: &str,
        user_id: Uuid,
    ) -> Result<Team, AppError> {
        let row = db_query!(
            self,
            "SELECT id, team_id, invitee_id, status FROM team_invitations
             WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("invitation not found".to_string()))?;
        let invitee_id = parse_uuid_str(row.try_get("invitee_id")?)?;
        if invitee_id != user_id {
            return Err(AppError::Forbidden(
                "invitation belongs to another user".to_string(),
            ));
        }
        if row.try_get::<String, _>("status")? != "pending" {
            return Err(AppError::Conflict(
                "invitation is no longer pending".to_string(),
            ));
        }
        let team_id = parse_uuid_str(row.try_get("team_id")?)?;
        let now = Utc::now();
        db_query!(
            self,
            "INSERT INTO team_members (team_id, user_id, role, joined_at)
             VALUES (?, ?, 'member', ?)",
        )
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if error.to_string().to_lowercase().contains("unique") {
                AppError::Conflict("user is already a team member".to_string())
            } else {
                AppError::Internal(error.to_string())
            }
        })?;
        db_query!(
            self,
            "UPDATE team_invitations SET status = 'accepted' WHERE id = ?"
        )
        .bind(row.try_get::<String, _>("id")?)
        .execute(&self.pool)
        .await?;
        self.get_team(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("team not found".to_string()))
    }

    pub(crate) async fn list_documents(
        &self,
        parent_id: Option<Uuid>,
        knowledge_base_id: Uuid,
    ) -> Result<Vec<Document>, AppError> {
        let rows = if let Some(parent_id) = parent_id {
            db_query!(self,
                "SELECT id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at
                 FROM documents WHERE knowledge_base_id = ? AND parent_id = ? ORDER BY LOWER(title)",
            )
                .bind(knowledge_base_id.to_string())
                .bind(parent_id.to_string())
                .fetch_all(&self.pool)
                .await?
        } else {
            db_query!(self,
                "SELECT id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at
                 FROM documents WHERE knowledge_base_id = ? AND parent_id IS NULL ORDER BY LOWER(title)",
            )
                .bind(knowledge_base_id.to_string())
                .fetch_all(&self.pool)
                .await?
        };
        rows.into_iter()
            .map(row_to_document_without_versions)
            .collect()
    }

    pub(crate) async fn all_documents_for_knowledge_base(
        &self,
        knowledge_base_id: Uuid,
    ) -> Result<Vec<Document>, AppError> {
        let rows = db_query!(self,
            "SELECT id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at
             FROM documents WHERE knowledge_base_id = ? ORDER BY LOWER(title)",
        )
            .bind(knowledge_base_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(row_to_document_without_versions)
            .collect()
    }

    pub(crate) async fn all_documents_for_search(&self) -> Result<Vec<Document>, AppError> {
        let rows = db_query!(
            self,
            "SELECT id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at
             FROM documents
             WHERE knowledge_base_id IS NOT NULL AND is_folder = 0
             ORDER BY updated_at DESC",
        )
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(row_to_document_without_versions)
            .collect()
    }

    pub(crate) async fn list_document_embedding_chunks(
        &self,
    ) -> Result<Vec<DocumentEmbeddingChunk>, AppError> {
        let rows = db_query!(
            self,
            "SELECT document_id, chunk_index, knowledge_base_id, content_hash, provider, model, dimensions, embedding
             FROM document_embedding_chunks",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(row_to_document_embedding_chunk)
            .collect()
    }

    pub(crate) async fn list_document_embeddings(
        &self,
    ) -> Result<Vec<DocumentEmbedding>, AppError> {
        let rows = db_query!(
            self,
            "SELECT document_id, knowledge_base_id, content_hash, provider, model, dimensions, embedding
             FROM document_embeddings",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_document_embedding).collect()
    }

    pub(crate) async fn delete_document_embedding_chunks(
        &self,
        document_id: Uuid,
    ) -> Result<(), AppError> {
        db_query!(
            self,
            "DELETE FROM document_embedding_chunks WHERE document_id = ?"
        )
        .bind(document_id.to_string())
        .execute(&self.pool)
        .await?;
        db_query!(
            self,
            "DELETE FROM document_embeddings WHERE document_id = ?"
        )
        .bind(document_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_document(&self, id: Uuid) -> Result<Option<Document>, AppError> {
        let row = db_query!(self,
            "SELECT id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at
             FROM documents WHERE id = ?",
        )
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let mut doc = row_to_document_without_versions(row)?;
        doc.versions = self.document_versions(id).await?;
        Ok(Some(doc))
    }

    pub(crate) async fn document_exists(&self, id: Uuid) -> Result<bool, AppError> {
        let row = db_query!(self, "SELECT 1 AS exists_flag FROM documents WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    pub(crate) async fn document_is_descendant(
        &self,
        candidate_id: Uuid,
        ancestor_id: Uuid,
    ) -> Result<bool, AppError> {
        let mut current_id = Some(candidate_id);
        let mut visited = HashSet::new();
        while let Some(id) = current_id {
            if id == ancestor_id {
                return Ok(true);
            }
            if !visited.insert(id) {
                return Ok(false);
            }
            let row = db_query!(self, "SELECT parent_id FROM documents WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;
            current_id = row
                .map(|row| row.try_get::<Option<String>, _>("parent_id"))
                .transpose()?
                .flatten()
                .map(parse_uuid_str)
                .transpose()?;
        }
        Ok(false)
    }

    pub(crate) async fn insert_document(&self, doc: &Document) -> Result<(), AppError> {
        db_query!(self,
            "INSERT INTO documents
             (id, knowledge_base_id, title, content, parent_id, is_folder, tags, author_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
            .bind(doc.id.to_string())
            .bind(doc.knowledge_base_id.to_string())
            .bind(&doc.title)
            .bind(&doc.content)
            .bind(doc.parent_id.map(|value| value.to_string()))
            .bind(i32::from(doc.is_folder))
            .bind(serde_json::to_string(&doc.tags)?)
            .bind(doc.author_id.to_string())
            .bind(doc.created_at.to_rfc3339())
            .bind(doc.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn update_document(&self, doc: &Document) -> Result<(), AppError> {
        db_query!(
            self,
            "UPDATE documents
             SET title = ?, content = ?, parent_id = ?, is_folder = ?, tags = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&doc.title)
        .bind(&doc.content)
        .bind(doc.parent_id.map(|value| value.to_string()))
        .bind(i32::from(doc.is_folder))
        .bind(serde_json::to_string(&doc.tags)?)
        .bind(doc.updated_at.to_rfc3339())
        .bind(doc.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn insert_document_version(
        &self,
        document_id: Uuid,
        version: &DocumentVersion,
    ) -> Result<(), AppError> {
        db_query!(
            self,
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

    pub(crate) async fn delete_document_tree(&self, id: Uuid) -> Result<(), AppError> {
        if !self.document_exists(id).await? {
            return Err(AppError::NotFound("document not found".to_string()));
        }
        let mut stack = vec![id];
        let mut ordered = Vec::new();
        let mut visited = HashSet::new();
        while let Some(current_id) = stack.pop() {
            if !visited.insert(current_id) {
                continue;
            }
            ordered.push(current_id);
            let child_rows = db_query!(self, "SELECT id FROM documents WHERE parent_id = ?")
                .bind(current_id.to_string())
                .fetch_all(&self.pool)
                .await?;
            for row in child_rows {
                stack.push(parse_uuid_str(row.try_get::<String, _>("id")?)?);
            }
        }
        for document_id in ordered.into_iter().rev() {
            db_query!(self, "DELETE FROM document_versions WHERE document_id = ?")
                .bind(document_id.to_string())
                .execute(&self.pool)
                .await?;
            self.delete_document_embedding_chunks(document_id).await?;
            db_query!(self, "DELETE FROM documents WHERE id = ?")
                .bind(document_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn document_versions(&self, document_id: Uuid) -> Result<Vec<DocumentVersion>, AppError> {
        let rows = db_query!(
            self,
            "SELECT content, saved_at FROM document_versions
             WHERE document_id = ? ORDER BY saved_at DESC",
        )
        .bind(document_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_document_version).collect()
    }
}

fn postgres_sql(sql: &'static str) -> &'static str {
    if !sql.contains('?') {
        return sql;
    }
    let cache = POSTGRES_QUERY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(normalized) = cache.get(sql) {
        return normalized;
    }
    let normalized = Box::leak(postgres_bind_sql(sql).into_boxed_str());
    cache.insert(sql, normalized);
    normalized
}

#[derive(Clone, Copy)]
enum SqlLexState {
    Code,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment,
}

fn postgres_bind_sql(sql: &str) -> String {
    let mut output = String::with_capacity(sql.len() + 16);
    let mut bind_index = 0usize;
    let mut state = SqlLexState::Code;
    let mut characters = sql.chars().peekable();

    while let Some(character) = characters.next() {
        match state {
            SqlLexState::Code => match character {
                '\'' => {
                    output.push(character);
                    state = SqlLexState::SingleQuoted;
                }
                '"' => {
                    output.push(character);
                    state = SqlLexState::DoubleQuoted;
                }
                '-' if characters.peek() == Some(&'-') => {
                    output.push(character);
                    output.push(characters.next().expect("line comment marker"));
                    state = SqlLexState::LineComment;
                }
                '/' if characters.peek() == Some(&'*') => {
                    output.push(character);
                    output.push(characters.next().expect("block comment marker"));
                    state = SqlLexState::BlockComment;
                }
                '?' => {
                    bind_index += 1;
                    output.push('$');
                    output.push_str(&bind_index.to_string());
                }
                _ => output.push(character),
            },
            SqlLexState::SingleQuoted => {
                output.push(character);
                if character == '\\' {
                    if let Some(escaped) = characters.next() {
                        output.push(escaped);
                    }
                } else if character == '\'' {
                    if characters.peek() == Some(&'\'') {
                        output.push(characters.next().expect("escaped quote"));
                    } else {
                        state = SqlLexState::Code;
                    }
                }
            }
            SqlLexState::DoubleQuoted => {
                output.push(character);
                if character == '"' {
                    if characters.peek() == Some(&'"') {
                        output.push(characters.next().expect("escaped identifier quote"));
                    } else {
                        state = SqlLexState::Code;
                    }
                }
            }
            SqlLexState::LineComment => {
                output.push(character);
                if character == '\n' {
                    state = SqlLexState::Code;
                }
            }
            SqlLexState::BlockComment => {
                output.push(character);
                if character == '*' && characters.peek() == Some(&'/') {
                    output.push(characters.next().expect("block comment terminator"));
                    state = SqlLexState::Code;
                }
            }
        }
    }

    output
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
        role: parse_role(row.try_get::<String, _>("role")?.as_str()).map_err(AppError::Internal)?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}

fn row_to_document_without_versions(row: sqlx::any::AnyRow) -> Result<Document, AppError> {
    let tags_json: String = row.try_get("tags")?;
    Ok(Document {
        id: parse_uuid_str(row.try_get::<String, _>("id")?)?,
        knowledge_base_id: parse_uuid_str(
            row.try_get::<Option<String>, _>("knowledge_base_id")?
                .ok_or_else(|| AppError::Internal("document has no knowledge base".to_string()))?,
        )?,
        title: row.try_get("title")?,
        content: row.try_get("content")?,
        parent_id: row
            .try_get::<Option<String>, _>("parent_id")?
            .map(parse_uuid_str)
            .transpose()?,
        is_folder: row.try_get::<i32, _>("is_folder")? != 0,
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

fn row_to_document_embedding(row: sqlx::any::AnyRow) -> Result<DocumentEmbedding, AppError> {
    let vector: Vec<f32> = from_str(row.try_get("embedding")?)?;
    let dimensions: i32 = row.try_get("dimensions")?;
    if dimensions < 0 || dimensions as usize != vector.len() {
        return Err(AppError::Internal(
            "stored embedding dimensions do not match vector data".to_string(),
        ));
    }
    Ok(DocumentEmbedding {
        document_id: parse_uuid_str(row.try_get("document_id")?)?,
        knowledge_base_id: parse_uuid_str(row.try_get("knowledge_base_id")?)?,
        content_hash: row.try_get("content_hash")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        vector,
    })
}

fn row_to_document_embedding_chunk(
    row: sqlx::any::AnyRow,
) -> Result<DocumentEmbeddingChunk, AppError> {
    let vector: Vec<f32> = from_str(row.try_get("embedding")?)?;
    let dimensions: i32 = row.try_get("dimensions")?;
    if dimensions < 0 || dimensions as usize != vector.len() {
        return Err(AppError::Internal(
            "stored embedding dimensions do not match vector data".to_string(),
        ));
    }
    let chunk_index: i32 = row.try_get("chunk_index")?;
    if chunk_index < 0 {
        return Err(AppError::Internal(
            "stored embedding chunk index is negative".to_string(),
        ));
    }
    Ok(DocumentEmbeddingChunk {
        document_id: parse_uuid_str(row.try_get("document_id")?)?,
        chunk_index: chunk_index as usize,
        knowledge_base_id: parse_uuid_str(row.try_get("knowledge_base_id")?)?,
        content_hash: row.try_get("content_hash")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        vector,
    })
}

fn row_to_mcp_token(row: sqlx::any::AnyRow) -> Result<McpTokenRecord, AppError> {
    Ok(McpTokenRecord {
        id: parse_uuid_str(row.try_get::<String, _>("id")?)?,
        user_id: parse_uuid_str(row.try_get::<String, _>("user_id")?)?,
        token_prefix: row.try_get("token_prefix")?,
        scope: row.try_get("scope")?,
        workspace_id: row
            .try_get::<Option<String>, _>("workspace_id")?
            .map(parse_uuid_str)
            .transpose()?,
        workspace_name: row.try_get("workspace_name")?,
        expires_at: parse_datetime_str(row.try_get("expires_at")?)?,
        revoked_at: row
            .try_get::<Option<String>, _>("revoked_at")?
            .map(parse_datetime_str)
            .transpose()?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}

fn parse_workspace_kind(value: &str) -> Result<WorkspaceKind, AppError> {
    match value {
        "personal" => Ok(WorkspaceKind::Personal),
        "team" => Ok(WorkspaceKind::Team),
        _ => Err(AppError::Internal(format!(
            "unknown workspace kind: {value}"
        ))),
    }
}

fn parse_team_role(value: String) -> Result<TeamRole, AppError> {
    match value.as_str() {
        "owner" => Ok(TeamRole::Owner),
        "admin" => Ok(TeamRole::Admin),
        "member" => Ok(TeamRole::Member),
        _ => Err(AppError::Internal(format!("unknown team role: {value}"))),
    }
}

fn row_to_workspace(row: sqlx::any::AnyRow) -> Result<Workspace, AppError> {
    Ok(Workspace {
        id: parse_uuid_str(row.try_get("id")?)?,
        name: row.try_get("name")?,
        kind: parse_workspace_kind(row.try_get::<String, _>("kind")?.as_str())?,
        owner_id: row
            .try_get::<Option<String>, _>("owner_id")?
            .map(parse_uuid_str)
            .transpose()?,
        team_id: row
            .try_get::<Option<String>, _>("team_id")?
            .map(parse_uuid_str)
            .transpose()?,
    })
}

fn row_to_knowledge_base(row: sqlx::any::AnyRow) -> Result<KnowledgeBase, AppError> {
    Ok(KnowledgeBase {
        id: parse_uuid_str(row.try_get("id")?)?,
        workspace_id: parse_uuid_str(row.try_get("workspace_id")?)?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}

fn row_to_team(row: sqlx::any::AnyRow) -> Result<Team, AppError> {
    Ok(Team {
        id: parse_uuid_str(row.try_get("id")?)?,
        name: row.try_get("name")?,
        owner_id: parse_uuid_str(row.try_get("owner_id")?)?,
        workspace_id: parse_uuid_str(row.try_get("workspace_id")?)?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}

fn row_to_team_member(row: sqlx::any::AnyRow) -> Result<TeamMember, AppError> {
    Ok(TeamMember {
        user_id: parse_uuid_str(row.try_get("user_id")?)?,
        username: row.try_get("username")?,
        display_name: row.try_get("display_name")?,
        role: parse_team_role(row.try_get("role")?)?,
        joined_at: parse_datetime_str(row.try_get("joined_at")?)?,
    })
}

fn row_to_invitation(row: sqlx::any::AnyRow) -> Result<TeamInvitation, AppError> {
    Ok(TeamInvitation {
        id: parse_uuid_str(row.try_get("id")?)?,
        team_id: parse_uuid_str(row.try_get("team_id")?)?,
        team_name: row.try_get("team_name")?,
        inviter_id: parse_uuid_str(row.try_get("inviter_id")?)?,
        invitee_id: parse_uuid_str(row.try_get("invitee_id")?)?,
        status: row.try_get("status")?,
        created_at: parse_datetime_str(row.try_get("created_at")?)?,
    })
}
