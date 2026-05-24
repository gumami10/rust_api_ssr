use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use rust_api_ssr::{
    app::create_router,
    cache::AppCache,
    handlers::{AppState, LoginRateLimiter, RequestMetrics},
    models::user::{SqliteUserRepository, User},
    services::{chat::ChatService, users::UserService},
};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceExt;

async fn test_app_with_pool() -> (Router, sqlx::SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect to in-memory sqlite");

    sqlx::query(
        r#"
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create users table");

    sqlx::query(
        r#"
        INSERT INTO users (name, email, password_hash)
        VALUES ('Alice', 'alice@example.com', 'alice-hash'), ('Bob', 'bob@example.com', 'bob-hash')
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed users");

    let cache = AppCache::new();
    let user_repo: Arc<dyn rust_api_ssr::models::user::UserRepository + Send + Sync> = Arc::new(
        rust_api_ssr::cache::CachedUserRepository::new(
            Arc::new(SqliteUserRepository::new(pool.clone())),
            cache.clone(),
        ),
    );
    let user_service = UserService::new(Arc::clone(&user_repo), cache.clone());
    let chat_service = ChatService::new(pool.clone(), cache.clone());
    let (chat_tx, _) = broadcast::channel(100);
    let app = create_router(AppState {
        user_repo,
        user_service,
        chat_service,
        pool: pool.clone(),
        chat_tx,
        request_metrics: RequestMetrics::default(),
        cookie_secure: false,
        login_rate_limiter: LoginRateLimiter::new(5, 900),
    });
    (app, pool)
}

async fn test_app() -> Router {
    test_app_with_pool().await.0
}

async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body)
}

async fn get_with_header(app: Router, uri: &str, header_name: &str, header_value: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header_name, header_value)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body)
}

async fn delete(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body)
}

#[tokio::test]
async fn list_users_returns_seeded_users_as_json() {
    let (status, body) = get(test_app().await, "/api/users").await;

    assert_eq!(status, StatusCode::OK);

    let mut users: Vec<User> = serde_json::from_slice(&body).expect("valid users json");
    users.sort_by_key(|user| user.id);

    assert_eq!(users.len(), 2);
    assert_eq!(users[0].name, "Alice");
    assert_eq!(users[0].email, "alice@example.com");
    assert_eq!(users[1].name, "Bob");
    assert_eq!(users[1].email, "bob@example.com");
}

#[tokio::test]
async fn get_user_returns_matching_user_as_json() {
    let (status, body) = get(test_app().await, "/api/users/1").await;

    assert_eq!(status, StatusCode::OK);

    let user: User = serde_json::from_slice(&body).expect("valid user json");
    assert_eq!(user.id, 1);
    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@example.com");
}

#[tokio::test]
async fn get_user_returns_not_found_json_for_missing_user() {
    let (status, body) = get(test_app().await, "/api/users/999").await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let error: Value = serde_json::from_slice(&body).expect("valid error json");
    assert_eq!(error, json!({ "error": "User with id 999 not found" }));
}

#[tokio::test]
async fn delete_user_returns_no_content_and_removes_user() {
    let app = test_app().await;
    let (status, body) = delete(app.clone(), "/api/users/1").await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());

    let (status, body) = get(app, "/api/users/1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let error: Value = serde_json::from_slice(&body).expect("valid error json");
    assert_eq!(error, json!({ "error": "User with id 1 not found" }));
}

#[tokio::test]
async fn index_page_renders_seeded_users() {
    let (status, body) = get(test_app().await, "/").await;

    assert_eq!(status, StatusCode::OK);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("<title>Users List</title>"));
    assert!(html.contains("<strong>Alice</strong>"));
    assert!(html.contains("alice@example.com"));
    assert!(html.contains("<strong>Bob</strong>"));
    assert!(html.contains("bob@example.com"));
}

#[tokio::test]
async fn cache_control_no_cache_bypasses_user_cache() {
    let (app, pool) = test_app_with_pool().await;

    // First request: warm the cache
    let (status, body) = get(app.clone(), "/api/users/1").await;
    assert_eq!(status, StatusCode::OK);
    let user: User = serde_json::from_slice(&body).expect("valid user json");
    assert_eq!(user.name, "Alice");

    // Update the user directly in the database
    sqlx::query("UPDATE users SET name = 'Alice Updated' WHERE id = 1")
        .execute(&pool)
        .await
        .expect("update user");

    // Second request without bypass: should still see cached "Alice"
    let (status, body) = get(app.clone(), "/api/users/1").await;
    assert_eq!(status, StatusCode::OK);
    let user: User = serde_json::from_slice(&body).expect("valid user json");
    assert_eq!(user.name, "Alice");

    // Third request with Cache-Control: no-cache: should see "Alice Updated"
    let (status, body) = get_with_header(app.clone(), "/api/users/1", "cache-control", "no-cache").await;
    assert_eq!(status, StatusCode::OK);
    let user: User = serde_json::from_slice(&body).expect("valid user json");
    assert_eq!(user.name, "Alice Updated");
}
