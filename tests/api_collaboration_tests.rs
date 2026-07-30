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

fn token(value: &Value) -> &str {
    value["token"].as_str().expect("auth response token")
}

#[tokio::test]
async fn collaboration_and_scoped_mcp_flow_works() {
    let filename = format!("api-collaboration-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app = gugle_rag::build_test_router(&database_url, "integration-test-secret")
        .await
        .unwrap();

    let (status, alice_auth) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "alice_collab",
            "password": "password123",
            "display_name": "Alice"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let alice_token = token(&alice_auth).to_string();

    let (status, workspaces) =
        json_request(&app, "GET", "/api/workspaces", Some(&alice_token), None).await;
    assert_eq!(status, StatusCode::OK);
    let personal_workspace = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["kind"] == "personal")
        .unwrap();
    let personal_kb = json_request(
        &app,
        "GET",
        &format!(
            "/api/workspaces/{}/knowledge-bases",
            personal_workspace["id"].as_str().unwrap()
        ),
        Some(&alice_token),
        None,
    )
    .await
    .1;
    let personal_kb_id = personal_kb[0]["id"].as_str().unwrap().to_string();

    let (status, team) = json_request(
        &app,
        "POST",
        "/api/teams",
        Some(&alice_token),
        Some(json!({ "name": "Platform Team" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let team_workspace_id = team["workspace_id"].as_str().unwrap();
    let (status, team_kbs) = json_request(
        &app,
        "GET",
        &format!("/api/workspaces/{team_workspace_id}/knowledge-bases"),
        Some(&alice_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let team_kb_id = team_kbs[0]["id"].as_str().unwrap().to_string();
    let (status, team_document) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&alice_token),
        Some(json!({
            "knowledge_base_id": team_kb_id,
            "title": "Rust team notes",
            "content": "Rust collaboration content"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let team_document_id = team_document["id"].as_str().unwrap().to_string();

    let (_, bob_auth) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "bob_collab",
            "password": "password123",
            "display_name": "Bob"
        })),
    )
    .await;
    let bob_token = token(&bob_auth).to_string();
    let (status, invitation) = json_request(
        &app,
        "POST",
        &format!("/api/teams/{}/invitations", team["id"].as_str().unwrap()),
        Some(&alice_token),
        Some(json!({ "username": "bob_collab" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let invite_token = invitation["invite_token"].as_str().unwrap();
    let (status, _) = json_request(
        &app,
        "POST",
        &format!("/api/invitations/{invite_token}/accept"),
        Some(&bob_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, bob_workspaces) =
        json_request(&app, "GET", "/api/workspaces", Some(&bob_token), None).await;
    assert!(
        bob_workspaces
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["id"] == team_workspace_id)
    );

    let (status, mcp) = json_request(
        &app,
        "POST",
        "/api/mcp/configs",
        Some(&alice_token),
        Some(json!({ "scope": "group", "team_id": team["id"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mcp["type"], "streamable-http");
    let mcp_path = url_path(mcp["url"].as_str().unwrap());
    let mcp_bearer = mcp["headers"]["Authorization"]
        .as_str()
        .unwrap()
        .trim_start_matches("Bearer ");

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({ "jsonrpc": "2.0", "id": 0, "method": "tools/list" })),
    )
    .await;
    for tool_name in [
        "search_knowledge",
        "read_document",
        "create_document",
        "update_document",
        "list_documents",
        "get_document_metadata",
    ] {
        let tool = rpc["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == tool_name)
            .unwrap();
        let required = tool["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&json!("workspace_id")));
        assert!(required.contains(&json!("knowledge_base_id")));
    }

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "list_workspaces", "arguments": {} }
        })),
    )
    .await;
    let scoped_workspaces = mcp_text_json(&rpc);
    assert_eq!(scoped_workspaces.as_array().unwrap().len(), 1);
    assert_eq!(scoped_workspaces[0]["id"], team_workspace_id);

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "list_knowledge_bases",
                "arguments": { "workspace_id": team_workspace_id }
            }
        })),
    )
    .await;
    let scoped_knowledge_bases = mcp_text_json(&rpc);
    assert_eq!(scoped_knowledge_bases[0]["id"], team_kb_id);

    let (status, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(
            mcp["headers"]["Authorization"]
                .as_str()
                .unwrap()
                .trim_start_matches("Bearer "),
        ),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": {
                    "workspace_id": team_workspace_id,
                    "knowledge_base_id": team_kb_id,
                    "query": "collaboration"
                }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        rpc["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Rust team notes")
    );

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "read_document",
                "arguments": {
                    "workspace_id": team_workspace_id,
                    "knowledge_base_id": team_kb_id,
                    "doc_id": team_document_id
                }
            }
        })),
    )
    .await;
    assert_eq!(mcp_text_json(&rpc)["workspace_id"], team_workspace_id);

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "read_document",
                "arguments": { "doc_id": team_document_id }
            }
        })),
    )
    .await;
    assert!(
        rpc["error"]["message"]
            .as_str()
            .unwrap()
            .contains("workspace_id")
    );

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "read_document",
                "arguments": {
                    "workspace_id": team_workspace_id,
                    "knowledge_base_id": personal_kb_id,
                    "doc_id": team_document_id
                }
            }
        })),
    )
    .await;
    assert!(
        rpc["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not belong")
    );
}

fn mcp_text_json(response: &Value) -> Value {
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn url_path(url: &str) -> String {
    let (_, host_and_path) = url.split_once("://").unwrap();
    let (_, path) = host_and_path.split_once('/').unwrap();
    format!("/{path}")
}
