use super::collaboration;
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
    pub(crate) knowledge_base_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) knowledge_base_id: Option<Uuid>,
}

pub(crate) async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let knowledge_base = resolve_knowledge_base(&state, user_id, query.knowledge_base_id).await?;
    Ok(Json(
        state
            .database
            .list_documents(query.parent_id, knowledge_base.id)
            .await?,
    ))
}

pub(crate) async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DocumentRequest>,
) -> Result<Json<Document>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let knowledge_base = resolve_knowledge_base(&state, user_id, input.knowledge_base_id).await?;
    let title = auth::require_non_empty(input.title, "title")?;
    let now = Utc::now();
    let doc = Document {
        id: Uuid::new_v4(),
        knowledge_base_id: knowledge_base.id,
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
        require_parent_in_knowledge_base(&state, parent_id, doc.knowledge_base_id).await?;
    }
    state.database.insert_document(&doc).await?;
    Ok(Json(doc))
}

pub(crate) async fn read_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Document>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
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
    let mut doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
    if input
        .knowledge_base_id
        .is_some_and(|knowledge_base_id| knowledge_base_id != doc.knowledge_base_id)
    {
        return Err(AppError::BadRequest(
            "moving documents between knowledge bases is not supported".to_string(),
        ));
    }
    if let Some(parent_id) = input.parent_id {
        if parent_id == id {
            return Err(AppError::BadRequest(
                "document cannot be its own parent".to_string(),
            ));
        }
        require_parent_in_knowledge_base(&state, parent_id, doc.knowledge_base_id).await?;
    }
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
    state.database.update_document(&doc).await?;
    Ok(Json(doc))
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
    let doc = state
        .database
        .get_document(id)
        .await?
        .ok_or_else(|| AppError::NotFound("document not found".to_string()))?;
    collaboration::require_knowledge_base_access(&state, user_id, doc.knowledge_base_id).await?;
    state.database.delete_document_tree(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resolve_knowledge_base(
    state: &AppState,
    user_id: Uuid,
    knowledge_base_id: Option<Uuid>,
) -> Result<crate::domain::KnowledgeBase, AppError> {
    match knowledge_base_id {
        Some(id) => collaboration::require_knowledge_base_access(state, user_id, id).await,
        None => collaboration::default_knowledge_base(state, user_id).await,
    }
}

async fn require_parent_in_knowledge_base(
    state: &AppState,
    parent_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<(), AppError> {
    let parent = state.database.get_document(parent_id).await?;
    if !matches!(parent, Some(ref doc) if doc.knowledge_base_id == knowledge_base_id) {
        return Err(AppError::BadRequest(
            "parent_id must exist in the same knowledge base".to_string(),
        ));
    }
    Ok(())
}
