use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::Row;
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

async fn register(app: &Router, username: &str) -> String {
    let (status, response) = json_request(
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
    .await;
    assert_eq!(status, StatusCode::OK);
    response["token"].as_str().unwrap().to_string()
}

async fn default_knowledge_base_id(app: &Router, token: &str) -> String {
    let (status, workspaces) = json_request(app, "GET", "/api/workspaces", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let workspace_id = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["kind"] == "personal")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, knowledge_bases) = json_request(
        app,
        "GET",
        &format!("/api/workspaces/{workspace_id}/knowledge-bases"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    knowledge_bases[0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn migration_rebuilds_the_vector_index_for_existing_documents() {
    let filename = format!("vector-migration-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let jwt_secret = "vector-migration-test-secret-long-enough";
    let old_app = gugle_rag::build_test_router(&database_url, jwt_secret)
        .await
        .unwrap();
    let token = register(&old_app, "vector_migration_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&old_app, &token).await;
    let (status, document) = json_request(
        &old_app,
        "POST",
        "/api/documents",
        Some(&token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "Migration notes",
            "content": "Existing documents receive a persistent vector embedding."
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let document_id = document["id"].as_str().unwrap().to_string();
    drop(old_app);

    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    sqlx::query("DROP TABLE document_embeddings")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let app = gugle_rag::build_test_router(&database_url, jwt_secret)
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM document_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    pool.close().await;

    let (status, results) = json_request(
        &app,
        "GET",
        &format!("/api/search?q=persistent&knowledge_base_id={knowledge_base_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results[0]["id"], document_id);

    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let row = sqlx::query(
        "SELECT provider, model, dimensions, embedding
         FROM document_embeddings WHERE document_id = ?",
    )
    .bind(&document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("provider"), "stub");
    assert_eq!(row.get::<String, _>("model"), "none");
    assert_eq!(row.get::<i32, _>("dimensions"), 384);
    assert_eq!(
        serde_json::from_str::<Vec<f32>>(row.get("embedding"))
            .unwrap()
            .len(),
        384
    );
    let first_hash = sqlx::query_scalar::<_, String>(
        "SELECT content_hash FROM document_embeddings WHERE document_id = ?",
    )
    .bind(&document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    pool.close().await;

    let (status, updated) = json_request(
        &app,
        "PUT",
        &format!("/api/documents/{document_id}"),
        Some(&token),
        Some(json!({
            "content": "The migrated document now contains refreshed material."
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, results) = json_request(
        &app,
        "GET",
        &format!("/api/search?q=refreshed&knowledge_base_id={knowledge_base_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results[0]["id"], document_id);

    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let second_hash = sqlx::query_scalar::<_, String>(
        "SELECT content_hash FROM document_embeddings WHERE document_id = ?",
    )
    .bind(&document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_ne!(first_hash, second_hash);
    pool.close().await;
}
