use gugle_rag::config::{
    DatabaseConfig, DatabaseEngine, SetupRequest, render_env_file, validate_database_url,
    validate_setup,
};

fn valid_setup() -> SetupRequest {
    SetupRequest {
        server_host: "127.0.0.1".to_string(),
        server_port: 8080,
        database_url: "sqlite://data/test.db".to_string(),
        jwt_secret: "a-secure-test-secret-that-is-long-enough".to_string(),
        registration_enabled: true,
        embedding_provider: "stub".to_string(),
        embedding_model: "none".to_string(),
        embedding_url: "https://api.siliconflow.cn/v1/embeddings".to_string(),
        vector_index_path: "data/vector-index".to_string(),
        siliconflow_url: None,
        siliconflow_api_key: None,
        reranker_enabled: true,
        reranker_provider: "custom_http".to_string(),
        reranker_model: "BAAI/bge-reranker-v2-m3".to_string(),
        reranker_url: "http://127.0.0.1:9000/rerank".to_string(),
        mcp_enabled: true,
        mcp_auth_required: false,
        mcp_public_url: None,
    }
}

#[test]
fn recognizes_all_supported_database_urls() {
    assert_eq!(
        validate_database_url("sqlite://data/app.db").unwrap(),
        DatabaseEngine::Sqlite
    );
    assert_eq!(
        validate_database_url("mysql://user:pass@localhost/app").unwrap(),
        DatabaseEngine::Mysql
    );
    assert_eq!(
        validate_database_url("postgresql://user:pass@localhost/app").unwrap(),
        DatabaseEngine::Postgres
    );
    assert!(validate_database_url("mongodb://localhost/app").is_err());
}

#[test]
fn sqlite_config_adds_create_mode_without_replacing_existing_query() {
    let plain = DatabaseConfig::from_url("sqlite://data/app.db".to_string());
    assert_eq!(plain.url, "sqlite://data/app.db?mode=rwc");

    let configured = DatabaseConfig::from_url("sqlite://data/app.db?mode=ro".to_string());
    assert_eq!(configured.url, "sqlite://data/app.db?mode=ro");
}

#[test]
fn database_config_redacts_passwords() {
    let config =
        DatabaseConfig::from_url("postgresql://gugle:private@localhost:5432/guglerag".to_string());
    assert_eq!(
        config.redacted_url(),
        "postgresql://gugle:*****@localhost:5432/guglerag"
    );
}

#[test]
fn setup_validation_and_env_rendering_include_reranker_settings() {
    let input = valid_setup();
    validate_setup(&input).unwrap();

    let output = render_env_file(&input);
    assert!(output.contains("DATABASE_URL=sqlite://data/test.db"));
    assert!(output.contains("REGISTRATION_ENABLED=true"));
    assert!(output.contains("RERANKER_ENABLED=true"));
    assert!(output.contains("RERANKER_PROVIDER=custom_http"));
    assert!(output.contains("RERANKER_MODEL=BAAI/bge-reranker-v2-m3"));
    assert!(output.contains("RERANKER_URL=http://127.0.0.1:9000/rerank"));
    assert!(output.contains("VECTOR_INDEX_PATH=data/vector-index"));
}

#[test]
fn siliconflow_embedding_fallback_uses_the_complete_endpoint() {
    let mut input = valid_setup();
    input.embedding_provider = "siliconflow".to_string();
    input.embedding_model = "BAAI/bge-m3".to_string();
    input.embedding_url.clear();
    input.siliconflow_url = Some("https://api.siliconflow.cn/".to_string());
    input.siliconflow_api_key = Some("test-key".to_string());

    let output = render_env_file(&input);
    assert!(output.contains("EMBEDDING_URL=https://api.siliconflow.cn/v1/embeddings"));
    assert!(!output.contains("//v1/embeddings"));
}

#[test]
fn siliconflow_reranker_requires_an_api_key() {
    let mut input = valid_setup();
    input.reranker_provider = "siliconflow".to_string();
    input.reranker_url.clear();

    let error = validate_setup(&input).unwrap_err();
    assert_eq!(
        error.to_string(),
        "SILICONFLOW_API_KEY is required for siliconflow reranking"
    );
}

#[test]
fn siliconflow_reranker_uses_the_base_url_without_a_custom_url() {
    let mut input = valid_setup();
    input.reranker_provider = "siliconflow".to_string();
    input.reranker_url.clear();
    input.siliconflow_api_key = Some("test-key".to_string());

    validate_setup(&input).unwrap();
}

#[test]
fn custom_http_reranker_requires_a_url() {
    let mut input = valid_setup();
    input.reranker_url.clear();
    let error = validate_setup(&input).unwrap_err();
    assert_eq!(
        error.to_string(),
        "RERANKER_URL is required for custom_http reranker"
    );
}
