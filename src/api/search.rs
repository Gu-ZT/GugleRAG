use crate::{AppState, auth, domain::SearchResult, error::AppError, search as search_engine};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct SearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<usize>,
}

pub(crate) async fn search_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    auth::require_user(&headers, &state).await?;
    let documents = state.database.all_documents().await?;
    Ok(Json(search_engine::search_documents(
        &documents,
        &query.q,
        query.limit.unwrap_or(10),
    )))
}
