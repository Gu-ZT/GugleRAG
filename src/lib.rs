pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod search;

mod api;
mod embedding;
pub mod logging;
mod mcp;
mod reranker;

use axum::Router;
use std::{net::SocketAddr, sync::Arc};
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
    let log_writer =
        logging::RollingLogWriter::new("logs").expect("failed to initialize logs/latest.log");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "gugle_rag=info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(log_writer)
                .with_ansi(false),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        log_file = "logs/latest.log",
        "starting GugleRAG"
    );

    sqlx::any::install_default_drivers();
    loop {
        let config = config::Config::from_env();
        info!(
            host = %config.host,
            port = config.port,
            database = %config.database.redacted_url(),
            setup_required = config.setup_required,
            mcp_enabled = config.mcp_enabled,
            embedding_provider = %config.embedding_provider,
            reranker_enabled = config.reranker_enabled,
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
            Ok(indexed_documents) => info!(indexed_documents, "document embedding index is ready"),
            Err(error) => {
                warn!("failed to rebuild document embeddings: {error}");
            }
        }

        let (restart_tx, mut restart_rx) = watch::channel(false);
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
        info!(address = %format!("http://{addr}"), "HTTP server is listening");
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
        match result {
            Ok(()) => info!("HTTP server stopped"),
            Err(error) => warn!("HTTP server stopped with error: {error}"),
        }
        break;
    }
}
