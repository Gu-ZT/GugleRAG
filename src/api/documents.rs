use crate::{
    AppState, auth,
    auth::can_edit,
    domain::{Document, DocumentVersion},
    error::AppError,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(crate) struct DocumentRequest {
    pub(crate) title: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) parent_id: Option<Uuid>,
}

pub(crate) async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    auth::require_user(&headers, &state).await?;
    Ok(Json(state.database.list_documents(query.parent_id).await?))
}

pub(crate) async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let title = auth::require_non_empty(input.title, "title")?;
    let now = Utc::now();
    let doc = Document {
        id: Uuid::new_v4(),
        title,
        content: input.content.unwrap_or_default(),
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

pub(crate) async fn read_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Document>, AppError> {
    auth::require_user(&headers, &state).await?;
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    Ok(Json(doc))
}

pub(crate) async fn update_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !can_edit(&user) {
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
        doc.title = auth::require_non_empty(Some(title), "title")?;
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

pub(crate) async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let user = state
        .database
        .get_user(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;
    if !can_edit(&user) {
        return Err(AppError::Forbidden("insufficient role".to_string()));
    }
    state.database.delete_document_tree(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
