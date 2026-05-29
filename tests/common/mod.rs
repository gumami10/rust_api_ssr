use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use rust_api_ssr::{
    app::create_router,
    cache::AppCache,
    handlers::{AppState, LoginRateLimiter, RequestMetrics},
    models::user::SqliteUserRepository,
    services::{chat::ChatService, users::UserService},
};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceExt;

pub async fn test_app_with_pool() -> (Router, sqlx::SqlitePool) {
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
        CREATE TABLE chat_rooms (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            created_by_user_id INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_rooms table");

    sqlx::query(
        r#"
        INSERT INTO chat_rooms (id, name, kind, created_by_user_id)
        VALUES (1, 'General', 'general', NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed general room");

    sqlx::query(
        r#"
        CREATE TABLE chat_room_members (
            room_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            joined_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (room_id, user_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_room_members table");

    sqlx::query(
        r#"
        CREATE TABLE chat_room_invites (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            room_id INTEGER NOT NULL,
            invited_user_id INTEGER NOT NULL,
            invited_by_user_id INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            accepted_at TEXT,
            UNIQUE (room_id, invited_user_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_room_invites table");

    sqlx::query(
        r#"
        CREATE TABLE chat_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            room_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            body TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            kind TEXT NOT NULL DEFAULT 'user',
            is_encrypted INTEGER NOT NULL DEFAULT 0,
            file_name TEXT,
            file_data BLOB,
            file_content_type TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_messages table");

    sqlx::query(
        r#"
        CREATE TABLE user_public_keys (
            user_id INTEGER PRIMARY KEY,
            public_key TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create user_public_keys table");

    sqlx::query(
        r#"
        CREATE TABLE chat_room_keys (
            room_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            encrypted_key TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (room_id, user_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_room_keys table");

    sqlx::query(
        r#"
        CREATE TABLE chat_room_read_positions (
            room_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            last_read_message_id INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (room_id, user_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create chat_room_read_positions table");

    let alice_hash =
        rust_api_ssr::services::users::hash_password("alice-password").expect("hash password");
    let bob_hash =
        rust_api_ssr::services::users::hash_password("bob-password").expect("hash password");

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

    let cache = AppCache::new();
    let user_repo: Arc<dyn rust_api_ssr::models::user::UserRepository + Send + Sync> =
        Arc::new(rust_api_ssr::cache::CachedUserRepository::new(
            Arc::new(SqliteUserRepository::new(pool.clone())),
            cache.clone(),
        ));
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

pub async fn test_app() -> Router {
    test_app_with_pool().await.0
}

pub async fn request(
    app: Router,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>) {
    let response = app.oneshot(request).await.expect("route request");
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().expect("valid location").to_string());
    let set_cookie: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().expect("valid cookie").to_string())
        .collect();
    let set_cookie = if set_cookie.is_empty() {
        None
    } else {
        Some(set_cookie.join(", "))
    };
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body, location, set_cookie)
}

pub async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request"),
    )
    .await;

    (status, body)
}

pub async fn post_form(app: Router, uri: &str, form: &str) -> (StatusCode, Vec<u8>, Option<String>) {
    let (status, body, location, _) = request(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.to_string()))
            .expect("build request"),
    )
    .await;

    (status, body, location)
}

pub fn cookie_value(set_cookie: &str) -> String {
    set_cookie
        .split(", ")
        .find(|s| s.trim().starts_with("chat_session="))
        .and_then(|s| s.split(';').next())
        .expect("chat_session cookie")
        .to_string()
}

pub fn csrf_token_from_cookie(set_cookie: &str) -> Option<String> {
    set_cookie
        .split(", ")
        .find(|s| s.trim().starts_with("csrf_token="))
        .and_then(|s| s.split(';').next())
        .map(|s| s.split_once('=').unwrap().1.to_string())
}
