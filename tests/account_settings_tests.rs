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

async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    json_request(
        app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "username": username, "password": password })),
    )
    .await
}

#[tokio::test]
async fn users_update_their_display_name_and_password() {
    let filename = format!("account-settings-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app = gugle_rag::build_test_router(
        &database_url,
        "account-settings-integration-secret-long-enough",
    )
    .await
    .unwrap();

    let (status, registered) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "username": "profile_user",
            "password": "password123",
            "display_name": "Original Name"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = registered["token"].as_str().unwrap();

    let (status, updated) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({ "display_name": "  New Display Name  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["display_name"], "New Display Name");
    assert_eq!(updated["username"], "profile_user");

    let (status, body) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({
            "display_name": "Should Not Persist",
            "current_password": "wrong-password",
            "new_password": "new-password-123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "current password is incorrect");
    let (status, unchanged) = json_request(&app, "GET", "/api/me", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["display_name"], "New Display Name");

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({
            "display_name": "New Display Name",
            "current_password": "password123",
            "new_password": "new-password-123"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        login(&app, "profile_user", "password123").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, logged_in) = login(&app, "profile_user", "new-password-123").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(logged_in["user"]["display_name"], "New Display Name");
}

#[tokio::test]
async fn profile_updates_validate_input_and_authentication() {
    let filename = format!("account-validation-{}.db", Uuid::new_v4());
    let database_url = format!("sqlite://data/{filename}?mode=rwc");
    let app = gugle_rag::build_test_router(
        &database_url,
        "account-validation-integration-secret-long-enough",
    )
    .await
    .unwrap();

    let (status, registered) = json_request(
        &app,
        "POST",
        "/api/auth/register",
        None,
        Some(json!({ "username": "validation_user", "password": "password123" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = registered["token"].as_str().unwrap();

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/me",
        None,
        Some(json!({ "display_name": "Anonymous" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({ "display_name": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({ "display_name": "a".repeat(121) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/me",
        Some(token),
        Some(json!({
            "display_name": "Validation User",
            "current_password": "password123",
            "new_password": "short"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
