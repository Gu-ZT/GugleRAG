pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod search;

mod api;
mod desktop;
mod embedding;
pub mod logging;
mod mcp;
mod reranker;
mod vector_store;

use crate::desktop::DesktopTray;
use axum::Router;
use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio::{net::TcpListener, sync::watch};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::{DefaultMakeSpan, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct AppState {
    pub(crate) database: db::Database,
    pub(crate) config: Arc<config::Config>,
    pub(crate) search: Arc<search::SearchEngine>,
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
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO))
                .on_failure(DefaultOnFailure::new().level(Level::WARN)),
        )
        .with_state(state)
}

pub async fn build_test_router(
    database_url: &str,
    jwt_secret: &str,
) -> Result<Router, error::AppError> {
    build_test_router_with_registration(database_url, jwt_secret, true).await
}

#[doc(hidden)]
pub async fn build_test_router_with_vector_index(
    database_url: &str,
    jwt_secret: &str,
    vector_index_path: &Path,
) -> Result<Router, error::AppError> {
    let mut config = config::Config::for_test(database_url.to_string(), jwt_secret.to_string());
    config.vector_index_path = vector_index_path.to_path_buf();
    build_test_router_from_config(config).await
}

pub async fn build_test_router_with_registration(
    database_url: &str,
    jwt_secret: &str,
    registration_enabled: bool,
) -> Result<Router, error::AppError> {
    let mut config = config::Config::for_test(database_url.to_string(), jwt_secret.to_string());
    config.registration_enabled = registration_enabled;
    build_test_router_from_config(config).await
}

#[doc(hidden)]
pub async fn build_test_router_with_retrieval(
    database_url: &str,
    jwt_secret: &str,
    embedding_url: &str,
    reranker_url: &str,
) -> Result<Router, error::AppError> {
    let mut config = config::Config::for_test(database_url.to_string(), jwt_secret.to_string());
    config.embedding_provider = "siliconflow".to_string();
    config.embedding_model = "test-embedding".to_string();
    config.embedding_url = embedding_url.to_string();
    config.siliconflow_api_key = "test-key".to_string();
    config.reranker_enabled = true;
    config.reranker_provider = "custom_http".to_string();
    config.reranker_model = "test-reranker".to_string();
    config.reranker_url = reranker_url.to_string();
    build_test_router_from_config(config).await
}

async fn build_test_router_from_config(config: config::Config) -> Result<Router, error::AppError> {
    sqlx::any::install_default_drivers();
    config::prepare_database_path(&config.database).await;
    let database = db::Database::connect(&config.database.url).await?;
    database.migrate().await?;
    let search = Arc::new(search::SearchEngine::from_config(
        &config,
        database.clone(),
    )?);
    let (restart_tx, _) = watch::channel(false);
    Ok(build_router(AppState {
        database,
        config: Arc::new(config),
        search,
        restart_tx,
    }))
}

pub async fn run() {
    let desktop_launch = desktop::is_desktop_launch();
    let log_writer =
        logging::RollingLogWriter::new("logs").expect("failed to initialize logs/latest.log");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "gugle_rag=info,tower_http=info".into());
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer()
            .with_writer(log_writer)
            .with_ansi(false),
    );
    if desktop_launch {
        subscriber.init();
    } else {
        subscriber
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
            .init();
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        log_file = "logs/latest.log",
        "starting GugleRAG"
    );

    sqlx::any::install_default_drivers();
    let (shutdown_tx, _) = watch::channel(false);
    let mut system_tray: Option<DesktopTray> = None;
    loop {
        let config = config::Config::from_env();
        info!(
            host = %config.host,
            port = config.port,
            database = %config.database.redacted_url(),
            setup_required = config.setup_required,
            mcp_enabled = config.mcp_enabled,
            embedding_provider = %config.embedding_provider,
            embedding_model = %config.embedding_model,
            embedding_url = %config.embedding_url,
            vector_index_path = %config.vector_index_path.display(),
            vector_database = %config
                .vector_database_redacted_url()
                .unwrap_or_else(|| "embedded-hnsw".to_string()),
            vector_store = config.vector_store_name(),
            reranker_enabled = config.reranker_enabled,
            reranker_provider = %config.reranker_provider,
            reranker_model = %config.reranker_model,
            "loaded runtime configuration"
        );
        config::prepare_database_path(&config.database).await;
        let database = db::Database::connect(&config.database.url)
            .await
            .expect("failed to connect database");
        info!("database connection established");
        database
            .migrate()
            .await
            .expect("failed to migrate database");
        info!("database migrations completed");
        let search = Arc::new(
            search::SearchEngine::from_config(&config, database.clone())
                .expect("failed to configure search engine"),
        );
        match search.reindex_all().await {
            Ok(indexed_documents) => info!(
                indexed_documents,
                vector_store = search.vector_store_name(),
                "document vector indexes are ready"
            ),
            Err(error) => {
                warn!("failed to rebuild document vector indexes: {error}");
            }
        }

        let (restart_tx, mut restart_rx) = watch::channel(false);
        let mut shutdown_rx = shutdown_tx.subscribe();
        let state = AppState {
            database: database.clone(),
            config: Arc::new(config.clone()),
            search,
            restart_tx: restart_tx.clone(),
        };
        let app = build_router(state);
        let addr: SocketAddr = format!("{}:{}", config.host, config.port)
            .parse()
            .expect("SERVER_HOST/SERVER_PORT must form a valid socket address");
        let listener = TcpListener::bind(addr)
            .await
            .expect("failed to bind configured address");
        let listener_addr = listener
            .local_addr()
            .expect("failed to read configured listener address");
        let listener_url = format!("http://{listener_addr}");
        info!(address = %listener_url, "HTTP server is listening");
        if desktop_launch {
            if let Some(system_tray) = &system_tray {
                system_tray.update_listener_url(&listener_url);
            } else {
                match DesktopTray::start(&listener_url, shutdown_tx.clone()) {
                    Ok(tray) => {
                        info!(address = %listener_url, "system tray is available");
                        system_tray = Some(tray);
                    }
                    Err(error) => warn!("failed to create system tray: {error}"),
                }
            }
        }
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            loop {
                let should_stop = tokio::select! {
                    result = restart_rx.changed() => result.is_err() || *restart_rx.borrow(),
                    result = shutdown_rx.changed() => result.is_err() || *shutdown_rx.borrow(),
                };
                if should_stop {
                    break;
                }
            }
        });
        let result = server.await;
        let restart_requested = *restart_tx.borrow();
        let shutdown_requested = *shutdown_tx.borrow();
        database.pool.close().await;
        if restart_requested && !shutdown_requested {
            info!("restarting GugleRAG with the saved configuration");
            continue;
        }
        if shutdown_requested {
            info!("shutting down after system tray exit");
        }
        match result {
            Ok(()) => info!("HTTP server stopped"),
            Err(error) => warn!("HTTP server stopped with error: {error}"),
        }
        break;
    }
}
