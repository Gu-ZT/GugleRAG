use crate::{
    domain::{Document, DocumentVersion, User, parse_role, role_to_str},
    error::AppError,
};
use chrono::{DateTime, Utc};
use sqlx::{AnyPool, Row, any::AnyPoolOptions};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    pub(crate) pool: AnyPool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self, AppError> {
        let pool = AnyPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub(crate) async fn migrate(&self) -> Result<(), AppError> {
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

    pub(crate) async fn user_count(&self) -> Result<i64, AppError> {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM app_users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<i64, _>("count")?)
    }

    pub(crate) async fn find_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    pub(crate) async fn get_user(&self, id: Uuid) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, display_name, password_hash, salt, role, created_at
             FROM app_users WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    pub(crate) async fn insert_user(&self, user: &User) -> Result<(), AppError> {
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

    pub(crate) async fn list_documents(
        &self,
        parent_id: Option<Uuid>,
    ) -> Result<Vec<Document>, AppError> {
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

    pub(crate) async fn all_documents(&self) -> Result<Vec<Document>, AppError> {
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

    pub(crate) async fn get_document(&self, id: Uuid) -> Result<Option<Document>, AppError> {
        let row = sqlx::query(
            "SELECT id, title, content, parent_id, tags, author_id, created_at, updated_at
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
        let row = sqlx::query("SELECT 1 AS exists_flag FROM documents WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    pub(crate) async fn insert_document(&self, doc: &Document) -> Result<(), AppError> {
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

    pub(crate) async fn update_document(&self, doc: &Document) -> Result<(), AppError> {
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

    pub(crate) async fn insert_document_version(
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

    pub(crate) async fn delete_document_tree(&self, id: Uuid) -> Result<(), AppError> {
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
