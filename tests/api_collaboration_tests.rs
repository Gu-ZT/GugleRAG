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
async fn deleting_a_knowledge_base_removes_its_documents() {
    let filename = format!("api-delete-kb-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app = gugle_rag::build_test_router(&database_url, "delete-kb-test-secret")
        .await
        .unwrap();

    let (_, auth) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "kb_deleter",
            "password": "password123",
            "display_name": "KB Deleter"
        })),
    )
    .await;
    let auth_token = token(&auth).to_string();

    let (_, workspaces) =
        json_request(&app, "GET", "/api/workspaces", Some(&auth_token), None).await;
    let workspace_id = workspaces[0]["id"].as_str().unwrap();
    let (status, knowledge_base) = json_request(
        &app,
        "POST",
        &format!("/api/workspaces/{workspace_id}/knowledge-bases"),
        Some(&auth_token),
        Some(json!({ "name": "Disposable KB", "description": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let knowledge_base_id = knowledge_base["id"].as_str().unwrap();

    let (status, document) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&auth_token),
        Some(json!({
            "knowledge_base_id": knowledge_base_id,
            "title": "Temporary document",
            "content": "Removed with its knowledge base"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let document_id = document["id"].as_str().unwrap();

    let (status, _) = json_request(
        &app,
        "DELETE",
        &format!("/api/knowledge-bases/{knowledge_base_id}"),
        Some(&auth_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, knowledge_bases) = json_request(
        &app,
        "GET",
        &format!("/api/workspaces/{workspace_id}/knowledge-bases"),
        Some(&auth_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        knowledge_bases
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != knowledge_base_id)
    );

    let (status, _) = json_request(
        &app,
        "GET",
        &format!("/api/documents/{document_id}"),
        Some(&auth_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
    let personal_workspace_id = personal_workspace["id"].as_str().unwrap().to_string();
    let personal_kb_id = personal_kb[0]["id"].as_str().unwrap().to_string();
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&alice_token),
        Some(json!({
            "knowledge_base_id": personal_kb_id,
            "title": "Personal federated notes",
            "content": "Personal federated search material"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

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
            "content": "Rust collaboration content with federated search material"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let team_document_id = team_document["id"].as_str().unwrap().to_string();
    let (status, team_archive) = json_request(
        &app,
        "POST",
        &format!("/api/workspaces/{team_workspace_id}/knowledge-bases"),
        Some(&alice_token),
        Some(json!({ "name": "Platform Archive", "description": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let team_archive_kb_id = team_archive["id"].as_str().unwrap().to_string();
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/documents",
        Some(&alice_token),
        Some(json!({
            "knowledge_base_id": team_archive_kb_id,
            "title": "Team archive federated notes",
            "content": "Team archive federated search material"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

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
        Some(json!({ "scope": "group", "workspace_id": team_workspace_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mcp["type"], "streamable-http");
    assert!(
        mcp["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/mcp/group/{team_workspace_id}"))
    );
    assert_eq!(
        mcp["headers"]["Authorization"],
        format!("Bearer {alice_token}")
    );
    let (status, repeated_mcp) = json_request(
        &app,
        "POST",
        "/api/mcp/configs",
        Some(&alice_token),
        Some(json!({ "scope": "group", "workspace_id": team_workspace_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated_mcp, mcp);

    let (status, all_mcp) = json_request(
        &app,
        "POST",
        "/api/mcp/configs",
        Some(&alice_token),
        Some(json!({ "scope": "all" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(all_mcp["url"].as_str().unwrap().ends_with("/mcp/all"));
    assert_eq!(
        all_mcp["headers"]["Authorization"],
        format!("Bearer {alice_token}")
    );
    let all_mcp_path = url_path(all_mcp["url"].as_str().unwrap());
    let (_, rpc) = json_request(
        &app,
        "POST",
        &all_mcp_path,
        Some(&alice_token),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "list_workspaces", "arguments": {} }
        })),
    )
    .await;
    let all_workspaces = mcp_text_json(&rpc);
    assert_eq!(all_workspaces.as_array().unwrap().len(), 2);
    assert!(
        all_workspaces
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["id"] == personal_workspace["id"])
    );
    assert!(
        all_workspaces
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["id"] == team_workspace_id)
    );

    let (_, rpc) = json_request(
        &app,
        "POST",
        &all_mcp_path,
        Some(&alice_token),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": {
                    "workspace_id": [personal_workspace_id.as_str(), team_workspace_id],
                    "knowledge_base_id": [
                        personal_kb_id.as_str(),
                        team_kb_id.as_str(),
                        team_archive_kb_id.as_str()
                    ],
                    "query": "federated"
                }
            }
        })),
    )
    .await;
    let multi_scope_search = mcp_text_json(&rpc);
    assert_eq!(multi_scope_search.as_array().unwrap().len(), 3);
    assert!(contains_search_result(
        &multi_scope_search,
        "Personal federated notes",
        &personal_workspace_id,
        &personal_kb_id,
    ));
    assert!(contains_search_result(
        &multi_scope_search,
        "Rust team notes",
        team_workspace_id,
        &team_kb_id,
    ));
    assert!(contains_search_result(
        &multi_scope_search,
        "Team archive federated notes",
        team_workspace_id,
        &team_archive_kb_id,
    ));

    let (_, rpc) = json_request(
        &app,
        "POST",
        &all_mcp_path,
        Some(&alice_token),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": {
                    "workspace_id": team_workspace_id,
                    "query": "federated"
                }
            }
        })),
    )
    .await;
    let all_team_knowledge_bases_search = mcp_text_json(&rpc);
    assert_eq!(all_team_knowledge_bases_search.as_array().unwrap().len(), 2);
    assert!(contains_search_result(
        &all_team_knowledge_bases_search,
        "Rust team notes",
        team_workspace_id,
        &team_kb_id,
    ));
    assert!(contains_search_result(
        &all_team_knowledge_bases_search,
        "Team archive federated notes",
        team_workspace_id,
        &team_archive_kb_id,
    ));

    let (_, rpc) = json_request(
        &app,
        "POST",
        &all_mcp_path,
        Some(&alice_token),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": {
                    "knowledge_base_id": [personal_kb_id.as_str(), team_archive_kb_id.as_str()],
                    "query": "federated"
                }
            }
        })),
    )
    .await;
    let all_workspace_search = mcp_text_json(&rpc);
    assert_eq!(all_workspace_search.as_array().unwrap().len(), 2);
    assert!(contains_search_result(
        &all_workspace_search,
        "Personal federated notes",
        &personal_workspace_id,
        &personal_kb_id,
    ));
    assert!(contains_search_result(
        &all_workspace_search,
        "Team archive federated notes",
        team_workspace_id,
        &team_archive_kb_id,
    ));

    let (_, rpc) = json_request(
        &app,
        "POST",
        &all_mcp_path,
        Some(&alice_token),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": { "query": "federated" }
            }
        })),
    )
    .await;
    let all_search = mcp_text_json(&rpc);
    assert_eq!(all_search.as_array().unwrap().len(), 3);

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
    let search_tool = rpc["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "search_knowledge")
        .unwrap();
    assert_eq!(search_tool["inputSchema"]["required"], json!(["query"]));
    for field in ["workspace_id", "knowledge_base_id"] {
        let options = search_tool["inputSchema"]["properties"][field]["oneOf"]
            .as_array()
            .unwrap();
        assert_eq!(options[0]["type"], "string");
        assert_eq!(options[1]["type"], "array");
    }
    for tool_name in [
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
    assert!(
        scoped_knowledge_bases
            .as_array()
            .unwrap()
            .iter()
            .any(|knowledge_base| knowledge_base["id"] == team_kb_id)
    );

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
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": { "query": "federated" }
            }
        })),
    )
    .await;
    let scoped_search = mcp_text_json(&rpc);
    assert_eq!(scoped_search.as_array().unwrap().len(), 2);
    assert!(contains_search_result(
        &scoped_search,
        "Rust team notes",
        team_workspace_id,
        &team_kb_id,
    ));
    assert!(contains_search_result(
        &scoped_search,
        "Team archive federated notes",
        team_workspace_id,
        &team_archive_kb_id,
    ));

    let (_, rpc) = json_request(
        &app,
        "POST",
        &mcp_path,
        Some(mcp_bearer),
        Some(json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "search_knowledge",
                "arguments": {
                    "workspace_id": [personal_workspace_id.as_str()],
                    "query": "federated"
                }
            }
        })),
    )
    .await;
    assert!(
        rpc["error"]["message"]
            .as_str()
            .unwrap()
            .contains("outside this MCP scope")
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

fn contains_search_result(
    results: &Value,
    title: &str,
    workspace_id: &str,
    knowledge_base_id: &str,
) -> bool {
    results
        .as_array()
        .map(|results| {
            results.iter().any(|result| {
                result["title"] == title
                    && result["workspace_id"] == workspace_id
                    && result["knowledge_base_id"] == knowledge_base_id
            })
        })
        .unwrap_or(false)
}

fn url_path(url: &str) -> String {
    let (_, host_and_path) = url.split_once("://").unwrap();
    let (_, path) = host_and_path.split_once('/').unwrap();
    format!("/{path}")
}
