mod auth;
mod documents;
mod search;
mod setup;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(setup::health))
        .route("/api/setup/status", get(setup::setup_status))
        .route("/api/setup", post(setup::save_setup))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/me", get(auth::me))
        .route(
            "/api/documents",
            get(documents::list_documents).post(documents::create_document),
        )
        .route(
            "/api/documents/{id}",
            get(documents::read_document)
                .put(documents::update_document)
                .delete(documents::delete_document),
        )
        .route("/api/search", get(search::search_documents))
}
