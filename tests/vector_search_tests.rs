use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::env;
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
    let vector_index_path =
        env::temp_dir().join(format!("guglerag-vector-migration-{}", Uuid::new_v4()));
    let old_app = gugle_rag::build_test_router_with_vector_index(
        &database_url,
        jwt_secret,
        &vector_index_path,
    )
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
    let legacy_pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let legacy_embedding = serde_json::to_string(&vec![0.0_f32; 384]).unwrap();
    let legacy_text = "Title: Migration notes\nTags: \n\nExisting documents receive a persistent vector embedding.";
    let legacy_content_hash = format!("{:x}", Sha256::digest(legacy_text.as_bytes()));
    sqlx::query(
        "INSERT INTO document_embeddings
         (document_id, knowledge_base_id, content_hash, provider, model, dimensions, embedding, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&document_id)
    .bind(&knowledge_base_id)
    .bind(legacy_content_hash)
    .bind("stub")
    .bind("none")
    .bind(384_i32)
    .bind(legacy_embedding)
    .bind("2026-01-01T00:00:00Z")
    .bind("2026-01-01T00:00:00Z")
    .execute(&legacy_pool)
    .await
    .unwrap();
    legacy_pool.close().await;
    drop(old_app);

    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    sqlx::query("DROP TABLE document_embedding_chunks")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let app = gugle_rag::build_test_router_with_vector_index(
        &database_url,
        jwt_secret,
        &vector_index_path,
    )
    .await
    .unwrap();
    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM document_embedding_chunks")
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
    let chunk_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_embedding_chunks WHERE document_id = ?")
            .bind(&document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(chunk_count, 0);
    let legacy_pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let legacy_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_embeddings WHERE document_id = ?")
            .bind(&document_id)
            .fetch_one(&legacy_pool)
            .await
            .unwrap();
    assert_eq!(legacy_count, 0);
    legacy_pool.close().await;
    let index_path = vector_index_path.join(format!("{knowledge_base_id}.index"));
    assert!(index_path.is_file());
    let first_index = std::fs::read(&index_path).unwrap();
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

    let second_index = std::fs::read(&index_path).unwrap();
    assert_ne!(first_index, second_index);

    drop(app);
    let app = gugle_rag::build_test_router_with_vector_index(
        &database_url,
        jwt_secret,
        &vector_index_path,
    )
    .await
    .unwrap();
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
}

#[tokio::test]
async fn long_documents_are_indexed_as_multiple_chunks() {
    let filename = format!("vector-chunks-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let jwt_secret = "vector-chunks-test-secret-long-enough";
    let vector_index_path =
        env::temp_dir().join(format!("guglerag-vector-chunks-{}", Uuid::new_v4()));
    let app = gugle_rag::build_test_router_with_vector_index(
        &database_url,
        jwt_secret,
        &vector_index_path,
    )
    .await
    .unwrap();
    let token = register(&app, "vector_chunks_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&app, &token).await;
    let content = format!("{} tail-only-marker", "prefix ".repeat(1200));
    let (status, document) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "Chunked document",
            "content": content
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{document}");
    let document_id = document["id"].as_str().unwrap();

    let (status, results) = json_request(
        &app,
        "GET",
        &format!("/api/search?q=tail-only-marker&knowledge_base_id={knowledge_base_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results[0]["id"], document_id);

    let index_path = vector_index_path.join(format!("{knowledge_base_id}.index"));
    assert!(index_path.is_file());
    let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM document_embedding_chunks WHERE document_id = ?")
            .bind(document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}
