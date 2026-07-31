use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::util::ServiceExt;
use uuid::Uuid;

async fn json_request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    let request = builder
        .header("Content-Type", "application/json")
        .body(Body::from(body.unwrap_or_else(|| json!({})).to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn register(app: &Router, username: &str) -> Value {
    json_request(
        app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": username,
            "password": "password123",
            "display_name": username
        })),
    )
    .await
    .1
}

fn token(auth: &Value) -> &str {
    auth["token"].as_str().expect("auth response token")
}

#[tokio::test]
async fn only_administrators_can_update_configuration_and_restart() {
    let filename = format!("admin-config-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let jwt_secret = "admin-config-integration-secret-long-enough";
    let app = gugle_rag::build_test_router(&database_url, jwt_secret)
        .await
        .unwrap();
    let admin_auth = register(&app, "admin_config_owner").await;
    let editor_auth = register(&app, "admin_config_editor").await;

    let (status, _) = json_request(&app, "GET", "/api/admin/config", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = json_request(
        &app,
        "GET",
        "/api/admin/config",
        Some(token(&editor_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, initial) = json_request(
        &app,
        "GET",
        "/api/admin/config",
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["restart_required"], false);
    assert_eq!(initial["secrets"]["jwt_secret_configured"], true);
    assert!(initial["current"].get("jwt_secret").is_none());
    assert!(initial["current"].get("siliconflow_api_key").is_none());
    let initial_vector_index_path = initial["current"]["vector_index_path"]
        .as_str()
        .expect("vector index path")
        .to_string();
    let env_path = initial["env_path"].as_str().expect("temporary env path");

    let payload = json!({
        "server_host": "127.0.0.1",
        "server_port": 9191,
        "database_url": database_url,
        "jwt_secret": "",
        "registration_enabled": false,
        "embedding_provider": "stub",
        "embedding_model": "none",
        "siliconflow_url": "https://api.siliconflow.cn",
        "siliconflow_api_key": "",
        "reranker_enabled": true,
        "reranker_provider": "custom_http",
        "reranker_model": "BAAI/bge-reranker-v2-m3",
        "reranker_url": "http://127.0.0.1:9000/rerank",
        "mcp_enabled": true,
        "mcp_auth_required": true,
        "mcp_public_url": "https://kb.example.com"
    });
    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/admin/config",
        Some(token(&editor_auth)),
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, saved) = json_request(
        &app,
        "PUT",
        "/api/admin/config",
        Some(token(&admin_auth)),
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["restart_required"], true);

    let env_file = tokio::fs::read_to_string(env_path).await.unwrap();
    assert!(env_file.contains(&format!("JWT_SECRET={jwt_secret}")));
    assert!(env_file.contains("REGISTRATION_ENABLED=false"));
    assert!(env_file.contains("SERVER_PORT=9191"));
    assert!(env_file.contains("RERANKER_ENABLED=true"));
    assert!(env_file.contains("MCP_PUBLIC_URL=https://kb.example.com"));
    assert!(env_file.contains("VECTOR_INDEX_PATH=\""));

    let (status, persisted) = json_request(
        &app,
        "GET",
        "/api/admin/config",
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(persisted["current"]["server_port"], 9191);
    assert_eq!(persisted["current"]["registration_enabled"], false);
    assert_eq!(
        persisted["current"]["vector_index_path"],
        initial_vector_index_path
    );
    assert_eq!(persisted["restart_required"], true);

    let (status, _) = json_request(
        &app,
        "POST",
        "/api/admin/restart",
        Some(token(&editor_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, restarted) = json_request(
        &app,
        "POST",
        "/api/admin/restart",
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(restarted["restarting"], true);

    tokio::fs::remove_file(env_path).await.unwrap();
}
