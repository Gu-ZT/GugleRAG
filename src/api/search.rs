use super::collaboration;
use crate::{AppState, auth, domain::SearchResult, error::AppError};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(crate) struct SearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<usize>,
    pub(crate) knowledge_base_id: Option<Uuid>,
}

pub(crate) async fn search_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    let user_id = auth::require_user(&headers, &state).await?;
    let knowledge_base = match query.knowledge_base_id {
        Some(id) => collaboration::require_knowledge_base_access(&state, user_id, id).await?,
        None => collaboration::default_knowledge_base(&state, user_id).await?,
    };
    let documents = state
        .database
        .all_documents_for_knowledge_base(knowledge_base.id)
        .await?;
    Ok(Json(
        state
            .search
            .search_documents(&documents, &query.q, query.limit.unwrap_or(10))
            .await?,
    ))
}
