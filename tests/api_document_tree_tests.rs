use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::io::{Cursor, Write};
use tower::util::ServiceExt;
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

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

async fn zip_request(
    app: &Router,
    uri: &str,
    token: &str,
    archive: Vec<u8>,
) -> (StatusCode, Value) {
    let boundary = "guglerag-zip-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"notes.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&archive);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
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

async fn test_app() -> Router {
    let filename = format!("api-document-tree-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    gugle_rag::build_test_router(&database_url, "document-tree-test-secret")
        .await
        .unwrap()
}

#[tokio::test]
async fn folders_form_a_tree_and_files_cannot_be_parents() {
    let app = test_app().await;
    let token = register(&app, "tree_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&app, &token).await;

    let (status, folder) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "Guides",
            "is_folder": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(folder["is_folder"], true);
    let folder_id = folder["id"].as_str().unwrap();

    let (status, file) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "intro.md",
            "content": "Welcome",
            "parent_id": folder_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{file}");
    assert_eq!(file["parent_id"], folder_id);
    assert_eq!(file["is_folder"], false);

    let (status, invalid_child) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "not-allowed.md",
            "parent_id": file["id"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(invalid_child["error"].as_str().unwrap().contains("folder"));

    let (status, tree) = json_request(
        &app,
        "GET",
        &format!("/api/documents?knowledge_base_id={knowledge_base_id}&tree=true"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tree.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn zip_import_preserves_folders_and_skips_binary_and_unsafe_entries() {
    let app = test_app().await;
    let token = register(&app, "zip_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&app, &token).await;
    let archive = zip_with_text_tree();

    let (status, import) = zip_request(
        &app,
        &format!("/api/knowledge-bases/{knowledge_base_id}/import-zip"),
        &token,
        archive,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{import}");
    assert_eq!(import["imported_files"], 2);
    assert_eq!(import["created_folders"], 2);
    assert_eq!(import["skipped_entries"], 2);

    let (status, tree) = json_request(
        &app,
        "GET",
        &format!("/api/documents?knowledge_base_id={knowledge_base_id}&tree=true"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = tree.as_array().unwrap();
    let guides = items.iter().find(|item| item["title"] == "guides").unwrap();
    let architecture = items
        .iter()
        .find(|item| item["title"] == "architecture")
        .unwrap();
    let overview = items
        .iter()
        .find(|item| item["title"] == "overview.md")
        .unwrap();
    assert_eq!(guides["is_folder"], true);
    assert_eq!(architecture["parent_id"], guides["id"]);
    assert_eq!(overview["parent_id"], architecture["id"]);
    assert_eq!(overview["content"], "Architecture notes");
    assert!(items.iter().all(|item| item["title"] != "binary.dat"));
    assert!(items.iter().all(|item| item["title"] != "outside.md"));
}

#[tokio::test]
async fn zip_import_requires_knowledge_base_access() {
    let app = test_app().await;
    let owner_token = register(&app, "zip_access_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&app, &owner_token).await;
    let other_token = register(&app, "zip_access_other").await;

    let (status, _) = zip_request(
        &app,
        &format!("/api/knowledge-bases/{knowledge_base_id}/import-zip"),
        &other_token,
        zip_with_text_tree(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

fn zip_with_text_tree() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.add_directory("guides/", options).unwrap();
    writer
        .add_directory("guides/architecture/", options)
        .unwrap();
    writer
        .start_file("guides/architecture/overview.md", options)
        .unwrap();
    writer.write_all(b"Architecture notes").unwrap();
    writer.start_file("README.md", options).unwrap();
    writer.write_all(b"Root notes").unwrap();
    writer.start_file("binary.dat", options).unwrap();
    writer.write_all(&[0, 159, 255]).unwrap();
    writer.start_file("../outside.md", options).unwrap();
    writer.write_all(b"must not import").unwrap();
    writer.finish().unwrap().into_inner()
}
