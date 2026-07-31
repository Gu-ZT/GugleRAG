use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{net::TcpListener, sync::Notify};
use tower::util::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct MockState {
    embedding_calls: Arc<AtomicUsize>,
    reranker_calls: Arc<AtomicUsize>,
}

async fn embedding_handler(
    State(state): State<MockState>,
    Json(request): Json<Value>,
) -> Json<Value> {
    state.embedding_calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(request["truncate"], json!("right"));
    let inputs = request["input"].as_array().unwrap();
    let data = inputs
        .iter()
        .map(|input| {
            let text = input.as_str().unwrap_or_default().to_lowercase();
            if text.contains("alpha") {
                json!({ "embedding": [1.0, 0.0] })
            } else {
                json!({ "embedding": [0.0, 1.0] })
            }
        })
        .collect::<Vec<_>>();
    Json(json!({ "data": data }))
}

async fn reranker_handler(
    State(state): State<MockState>,
    Json(request): Json<Value>,
) -> Json<Value> {
    state.reranker_calls.fetch_add(1, Ordering::SeqCst);
    let documents = request["documents"].as_array().unwrap();
    let results = if documents.len() >= 2 {
        vec![
            json!({ "index": 1, "relevance_score": 0.99 }),
            json!({ "index": 0, "relevance_score": 0.10 }),
        ]
    } else {
        vec![json!({ "index": 0, "relevance_score": 0.99 })]
    };
    Json(json!({ "results": results }))
}

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
    let workspace_id = workspaces[0]["id"].as_str().unwrap();
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
async fn configured_embedding_and_reranker_endpoints_are_called() {
    let state = MockState {
        embedding_calls: Arc::new(AtomicUsize::new(0)),
        reranker_calls: Arc::new(AtomicUsize::new(0)),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mock_app = Router::new()
        .route("/v1/embeddings", post(embedding_handler))
        .route("/rerank", post(reranker_handler))
        .with_state(state.clone());
    let shutdown = Arc::new(Notify::new());
    let shutdown_signal = shutdown.clone();
    let mock_server = tokio::spawn(async move {
        axum::serve(listener, mock_app)
            .with_graceful_shutdown(async move { shutdown_signal.notified().await })
            .await
            .unwrap();
    });

    let filename = format!("retrieval-provider-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let base_url = format!("http://{address}");
    let app = gugle_rag::build_test_router_with_retrieval(
        &database_url,
        "retrieval-provider-test-secret-long-enough",
        &format!("{base_url}/v1/embeddings"),
        &format!("{base_url}/rerank"),
    )
    .await
    .unwrap();
    let token = register(&app, "retrieval_provider_owner").await;
    let knowledge_base_id = default_knowledge_base_id(&app, &token).await;

    for (title, content) in [("Alpha", "alpha material"), ("Beta", "beta material")] {
        let (status, response) = json_request(
            &app,
            "POST",
            "/api/documents",
            Some(&token),
            Some(json!({
                "knowledge_base_id": knowledge_base_id,
                "title": title,
                "content": content
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{response}");
    }

    let (status, results) = json_request(
        &app,
        "GET",
        &format!("/api/search?q=alpha&limit=1&knowledge_base_id={knowledge_base_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert_eq!(results[0]["title"], "Beta");
    assert_eq!(state.embedding_calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.reranker_calls.load(Ordering::SeqCst), 1);

    shutdown.notify_one();
    mock_server.await.unwrap();
}
