pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod search;

mod api;
mod mcp;

use axum::Router;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub(crate) database: db::Database,
    pub(crate) config: Arc<config::Config>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(api::router())
        .merge(mcp::router())
        .fallback_service(
            ServeDir::new("frontend/dist")
                .not_found_service(ServeFile::new("frontend/dist/index.html")),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "guglerag=info,tower_http=info".into()),
        )
        .init();

    sqlx::any::install_default_drivers();
    let config = config::Config::from_env();
    config::prepare_database_path(&config.database).await;
    let database = db::Database::connect(&config.database.url)
        .await
        .expect("failed to connect database");
    database
        .migrate()
        .await
        .expect("failed to migrate database");

    let state = AppState {
        database,
        config: Arc::new(config.clone()),
    };
    let app = build_router(state);
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("SERVER_HOST/SERVER_PORT must form a valid socket address");
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind configured address");
    info!("GugleRAG listening on http://{addr}");
    if let Err(error) = axum::serve(listener, app).await {
        warn!("server stopped: {error}");
    }
}
