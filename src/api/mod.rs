mod admin;
mod auth;
mod collaboration;
mod documents;
mod search;
mod setup;

use crate::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(setup::health))
        .route("/api/setup/status", get(setup::setup_status))
        .route("/api/setup", post(setup::save_setup))
        .route(
            "/api/admin/config",
            get(admin::get_config).put(admin::save_config),
        )
        .route("/api/admin/restart", post(admin::restart))
        .route(
            "/api/admin/users",
            get(admin::list_users).post(admin::create_user),
        )
        .route(
            "/api/admin/users/{user_id}",
            put(admin::update_user).delete(admin::delete_user),
        )
        .route("/api/auth/register", post(auth::register))
        .route(
            "/api/auth/registration-status",
            get(auth::registration_status),
        )
        .route("/api/auth/login", post(auth::login))
        .route("/api/me", get(auth::me))
        .route("/api/workspaces", get(collaboration::list_workspaces))
        .route(
            "/api/workspaces/{workspace_id}/knowledge-bases",
            get(collaboration::list_knowledge_bases).post(collaboration::create_knowledge_base),
        )
        .route(
            "/api/teams",
            get(collaboration::list_teams).post(collaboration::create_team),
        )
        .route(
            "/api/teams/{team_id}/members",
            get(collaboration::list_team_members),
        )
        .route(
            "/api/teams/{team_id}/invitations",
            post(collaboration::create_invitation),
        )
        .route("/api/invitations", get(collaboration::list_invitations))
        .route(
            "/api/invitations/{token}/accept",
            post(collaboration::accept_invitation),
        )
        .route("/api/mcp/configs", post(collaboration::create_mcp_config))
        .route(
            "/api/knowledge-bases/{knowledge_base_id}",
            delete(collaboration::delete_knowledge_base),
        )
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
        .route(
            "/api/knowledge-bases/{knowledge_base_id}/import-zip",
            post(documents::import_zip)
                .layer(DefaultBodyLimit::max(documents::MAX_ZIP_MULTIPART_BYTES)),
        )
        .route("/api/search", get(search::search_documents))
}
