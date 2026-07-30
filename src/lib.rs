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
use tokio::{net::TcpListener, sync::watch};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub(crate) database: db::Database,
    pub(crate) config: Arc<config::Config>,
    pub(crate) restart_tx: watch::Sender<bool>,
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

pub async fn build_test_router(
    database_url: &str,
    jwt_secret: &str,
) -> Result<Router, error::AppError> {
    sqlx::any::install_default_drivers();
    let config = config::Config::for_test(database_url.to_string(), jwt_secret.to_string());
    config::prepare_database_path(&config.database).await;
    let database = db::Database::connect(&config.database.url).await?;
    database.migrate().await?;
    let (restart_tx, _) = watch::channel(false);
    Ok(build_router(AppState {
        database,
        config: Arc::new(config),
        restart_tx,
    }))
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "guglerag=info,tower_http=info".into()),
        )
        .init();

    sqlx::any::install_default_drivers();
    loop {
        let config = config::Config::from_env();
        config::prepare_database_path(&config.database).await;
        let database = db::Database::connect(&config.database.url)
            .await
            .expect("failed to connect database");
        database
            .migrate()
            .await
            .expect("failed to migrate database");

        let (restart_tx, mut restart_rx) = watch::channel(false);
        let state = AppState {
            database: database.clone(),
            config: Arc::new(config.clone()),
            restart_tx: restart_tx.clone(),
        };
        let app = build_router(state);
        let addr: SocketAddr = format!("{}:{}", config.host, config.port)
            .parse()
            .expect("SERVER_HOST/SERVER_PORT must form a valid socket address");
        let listener = TcpListener::bind(addr)
            .await
            .expect("failed to bind configured address");
        info!("GugleRAG listening on http://{addr}");
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            while restart_rx.changed().await.is_ok() {
                if *restart_rx.borrow() {
                    break;
                }
            }
        });
        let result = server.await;
        let restart_requested = *restart_tx.borrow();
        database.pool.close().await;
        if restart_requested {
            info!("restarting GugleRAG with the saved configuration");
            continue;
        }
        if let Err(error) = result {
            warn!("server stopped: {error}");
        }
        break;
    }
}
