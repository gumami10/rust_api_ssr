use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use rust_api_ssr::{
    app::create_router,
    handlers::AppState,
    models::user::SqliteUserRepository,
    services::users::hash_password,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceExt;

async fn test_app() -> Router {
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
        CREATE TABLE sessions (
            token TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create sessions table");

    sqlx::query(
        r#"
        CREATE TABLE chat_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_messages table");

    let alice_hash = hash_password("alice-password").expect("hash password");
    let bob_hash = hash_password("bob-password").expect("hash password");

    sqlx::query(
        r#"
        INSERT INTO users (name, email, password_hash)
        VALUES (?, ?, ?), (?, ?, ?)
        "#,
    )
    .bind("Alice")
    .bind("alice@example.com")
    .bind(alice_hash)
    .bind("Bob")
    .bind("bob@example.com")
    .bind(bob_hash)
    .execute(&pool)
    .await
    .expect("seed users");

    let user_repo = Arc::new(SqliteUserRepository::new(pool.clone()));
    let (chat_tx, _) = broadcast::channel(100);
    create_router(AppState {
        user_repo,
        pool,
        chat_tx,
    })
}

async fn request(app: Router, request: Request<Body>) -> (StatusCode, Vec<u8>, Option<String>) {
    let response = app.oneshot(request).await.expect("route request");
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().expect("valid location").to_string());
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body, location)
}

async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let (status, body, _) = request(
        app,
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request"),
    )
    .await;

    (status, body)
}

async fn post_form(app: Router, uri: &str, form: &str) -> (StatusCode, Vec<u8>, Option<String>) {
    request(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.to_string()))
            .expect("build request"),
    )
    .await
}

#[tokio::test]
async fn health_and_readiness_endpoints_return_no_content() {
    for uri in ["/healthz", "/readyz"] {
        let (status, body) = get(test_app().await, uri).await;

        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(body.is_empty());
    }
}

#[tokio::test]
async fn users_index_renders_at_root_and_canonical_path() {
    for uri in ["/", "/users"] {
        let (status, body) = get(test_app().await, uri).await;

        assert_eq!(status, StatusCode::OK);

        let html = String::from_utf8(body).expect("valid utf-8 html");
        assert!(html.contains("<title>Users List</title>"));
        assert!(html.contains(r#"<a href="/users/1"><strong>Alice</strong></a>"#));
        assert!(html.contains("alice@example.com"));
        assert!(html.contains(r#"<a href="/users/2"><strong>Bob</strong></a>"#));
        assert!(html.contains("bob@example.com"));
    }
}

#[tokio::test]
async fn user_detail_renders_existing_user() {
    let (status, body) = get(test_app().await, "/users/1").await;

    assert_eq!(status, StatusCode::OK);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("<h1>Alice</h1>"));
    assert!(html.contains("alice@example.com"));
    assert!(html.contains(r#"href="/users/1/edit""#));
}

#[tokio::test]
async fn user_detail_returns_not_found_for_missing_user() {
    let (status, _) = get(test_app().await, "/users/999").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn new_user_form_renders_validation_errors() {
    let (status, body) = get(test_app().await, "/users/new").await;

    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)
        .expect("valid utf-8 html")
        .contains(r#"<form method="post" action="/users">"#));

    let (status, body, location) =
        post_form(test_app().await, "/users", "name=&email=invalid&password=").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(location, None);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Name is required."));
    assert!(html.contains("Email must contain @."));
    assert!(html.contains("Password is required."));
}

#[tokio::test]
async fn valid_create_redirects_to_created_user() {
    let (status, _, location) = post_form(
        test_app().await,
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-pass",
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users/3"));
}

#[tokio::test]
async fn edit_user_form_updates_existing_user() {
    let (status, body) = get(test_app().await, "/users/1/edit").await;

    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)
        .expect("valid utf-8 html")
        .contains(r#"<form method="post" action="/users/1">"#));

    let app = test_app().await;
    let (status, _, location) = post_form(
        app.clone(),
        "/users/1",
        "name=Alice%20Updated&email=alice.updated%40example.com",
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users/1"));

    let (status, body) = get(app, "/users/1").await;
    assert_eq!(status, StatusCode::OK);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("<h1>Alice Updated</h1>"));
    assert!(html.contains("alice.updated@example.com"));
}

#[tokio::test]
async fn edit_validation_rerenders_form() {
    let (status, body, location) =
        post_form(test_app().await, "/users/1", "name=Alice&email=").await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(location, None);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Email is required."));
    assert!(html.contains(r#"<form method="post" action="/users/1">"#));
}

#[tokio::test]
async fn create_validation_rerenders_form_for_duplicate_email() {
    let (status, body, location) = post_form(
        test_app().await,
        "/users",
        "name=Carol&email=alice%40example.com&password=carol-pass",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(location, None);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Email already exists."));
    assert!(html.contains(r#"<form method="post" action="/users">"#));
}

#[tokio::test]
async fn update_validation_rerenders_form_for_duplicate_email() {
    let (status, body, location) = post_form(
        test_app().await,
        "/users/1",
        "name=Alice&email=bob%40example.com",
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(location, None);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Email already exists."));
    assert!(html.contains(r#"<form method="post" action="/users/1">"#));
}

#[tokio::test]
async fn login_redirects_into_chat_and_sets_a_session_cookie() {
    let (status, _, location) = post_form(
        test_app().await,
        "/login",
        "email=alice%40example.com&password=alice-password",
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/chat"));
}

#[tokio::test]
async fn chat_page_redirects_anonymous_users() {
    let (status, _) = get(test_app().await, "/chat").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn delete_user_redirects_back_to_index() {
    let app = test_app().await;
    let (status, _, location) = post_form(app.clone(), "/users/1/delete", "").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users"));

    let (status, body) = get(app, "/users/1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains(r#""error":"User with id 1 not found""#));
}
