use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, from_slice, json};
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
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        from_slice(&bytes).unwrap()
    };
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

async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    json_request(
        app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({
            "username": username,
            "password": password
        })),
    )
    .await
}

fn token(auth: &Value) -> &str {
    auth["token"].as_str().expect("auth response token")
}

#[tokio::test]
async fn closed_registration_rejects_public_signup() {
    let filename = format!("registration-closed-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app = gugle_rag::build_test_router_with_registration(
        &database_url,
        "registration-closed-secret-long-enough",
        false,
    )
    .await
    .unwrap();

    let (status, bootstrap) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "bootstrap_admin",
            "password": "password123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bootstrap["user"]["role"], "admin");

    let (status, body) =
        json_request(&app, "GET", "/api/auth/registration-status", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["registration_enabled"], false);

    let (status, body) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "closed_signup",
            "password": "password123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("registration is closed")
    );
}

#[tokio::test]
async fn administrators_manage_users_and_their_workspace_overview() {
    let filename = format!("admin-users-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app =
        gugle_rag::build_test_router(&database_url, "admin-user-integration-secret-long-enough")
            .await
            .unwrap();
    let admin_auth = register(&app, "user_admin").await;
    let editor_auth = register(&app, "user_editor").await;

    let (status, _) = json_request(
        &app,
        "GET",
        "/api/admin/users",
        Some(token(&editor_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, users) = json_request(
        &app,
        "GET",
        "/api/admin/users",
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let users = users.as_array().unwrap();
    assert_eq!(users.len(), 2);
    assert!(users.iter().any(|user| user["username"] == "user_editor"));
    let editor = users
        .iter()
        .find(|user| user["username"] == "user_editor")
        .unwrap();
    assert_eq!(editor["workspaces"].as_array().unwrap().len(), 1);

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/admin/users",
        Some(token(&admin_auth)),
        Some(json!({
            "username": "managed_user",
            "password": "managed-password",
            "display_name": "Managed User",
            "role": "reader"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["role"], "reader");
    assert_eq!(created["workspaces"].as_array().unwrap().len(), 1);
    let managed_user_id = created["id"].as_str().unwrap();

    let (status, updated) = json_request(
        &app,
        "PUT",
        &format!("/api/admin/users/{managed_user_id}"),
        Some(token(&admin_auth)),
        Some(json!({
            "username": "managed_renamed",
            "password": "changed-password",
            "display_name": "Managed Renamed",
            "role": "editor"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["username"], "managed_renamed");
    assert_eq!(updated["role"], "editor");

    let (status, _) = login(&app, "managed_user", "managed-password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, logged_in) = login(&app, "managed_renamed", "changed-password").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(logged_in["user"]["role"], "editor");

    let admin_user_id = admin_auth["user"]["id"].as_str().unwrap();
    let (status, _) = json_request(
        &app,
        "DELETE",
        &format!("/api/admin/users/{admin_user_id}"),
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = json_request(
        &app,
        "DELETE",
        &format!("/api/admin/users/{managed_user_id}"),
        Some(token(&admin_auth)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = login(&app, "managed_renamed", "changed-password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
