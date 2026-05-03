use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use rust_api_ssr::{
    app::create_router,
    handlers::{AppState, RequestMetrics},
    models::user::SqliteUserRepository,
    services::users::hash_password,
};
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
    let app = create_router(AppState {
        user_repo,
        pool: pool.clone(),
        chat_tx,
        request_metrics: RequestMetrics::default(),
    });
    (app, pool)
}

async fn test_app() -> Router {
    test_app_with_pool().await.0
}

async fn request(
    app: Router,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>) {
    let response = app.oneshot(request).await.expect("route request");
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().expect("valid location").to_string());
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|value| value.to_str().expect("valid cookie").to_string());
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body")
        .to_vec();

    (status, body, location, set_cookie)
}

async fn get(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
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

async fn post_form(app: Router, uri: &str, form: &str) -> (StatusCode, Vec<u8>, Option<String>) {
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

fn cookie_value(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .expect("cookie value")
        .to_string()
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
async fn valid_create_logs_the_new_user_in() {
    let app = test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/users")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "name=Carol&email=carol%40example.com&password=carol-pass".to_string(),
        ))
        .expect("build request");

    let (status, _, location, set_cookie) = request(app.clone(), req).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users/3"));

    let cookie = cookie_value(set_cookie.as_deref().expect("set cookie"));
    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri("/users/3")
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .expect("build request"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Logout"));
    assert!(html.contains("See profile: Carol"));
    assert!(!html.contains(r#"href="/login">Login"#));
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
    let app = test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice%40example.com&password=alice-password".to_string(),
        ))
        .expect("build request");

    let (status, _, location, set_cookie) = request(app.clone(), req).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/chat"));
    assert!(set_cookie.is_some());

    let cookie = cookie_value(set_cookie.as_deref().expect("set cookie"));
    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri("/chat")
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .expect("build request"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).expect("valid utf-8 html");
    assert!(html.contains("Logout"));
    assert!(html.contains("See profile: Alice"));
    assert!(html.contains("General"));
    assert!(html.contains("Create private room"));
}

#[tokio::test]
async fn create_private_room_redirects_to_the_new_room() {
    let app = test_app().await;
    let login_req = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice%40example.com&password=alice-password".to_string(),
        ))
        .expect("build request");

    let (_, _, _, set_cookie) = request(app.clone(), login_req).await;
    let cookie = cookie_value(set_cookie.as_deref().expect("set cookie"));

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from("name=Pair%20Room&participant_ids=2".to_string()))
        .expect("build request");

    let (status, _, location, _) = request(app, create_req).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/chat/rooms/2"));
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

#[tokio::test]
async fn login_form_validation_errors() {
    let app = test_app().await;

    // Empty email
    let (status, body, _) = post_form(app.clone(), "/login", "email=&password=pass").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body).unwrap().contains("Email is required."));

    // Invalid email
    let (status, body, _) = post_form(app.clone(), "/login", "email=invalid&password=pass").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body).unwrap().contains("Email must contain @."));

    // Empty password
    let (status, body, _) =
        post_form(app.clone(), "/login", "email=alice@example.com&password=").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body).unwrap().contains("Password is required."));

    // Incorrect credentials
    let (status, body, _) = post_form(
        app.clone(),
        "/login",
        "email=alice@example.com&password=wrong",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Email or password is incorrect."));
}

#[tokio::test]
async fn logout_invalidates_session() {
    let app = test_app().await;

    // Login
    let login_req = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice@example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_req).await;
    let cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Verify chat access
    let (status, _, _, _) = request(
        app.clone(),
        Request::builder()
            .uri("/chat")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Logout
    let (status, _, location, set_cookie) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/logout")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users"));
    assert!(set_cookie.unwrap().contains("Max-Age=0"));

    // Verify chat access is gone
    let (status, _, _, _) = request(
        app.clone(),
        Request::builder()
            .uri("/chat")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER); // Redirect to login
}

#[tokio::test]
async fn create_room_validation_errors() {
    let app = test_app().await;

    // Login
    let login_req = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice@example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_req).await;
    let cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Create room with no other participants
    let (status, body, location, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/rooms")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, cookie)
            .body(Body::from("name=Empty&participant_ids="))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK); // Renders the page with error
    assert_eq!(location, None);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Add at least one other participant"));
}

#[tokio::test]
async fn invite_and_accept_flow() {
    let app = test_app().await;

    // Alice login
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice@example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Alice creates a private room with Bob (Bob is ID 2)
    let create_room = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_room).await;
    let room_path = location.unwrap();

    // Create a new user Carol to invite.
    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;
    // Carol should be ID 3.

    // Alice invites Carol to the room
    let invite_req = Request::builder()
        .method("POST")
        .uri(format!("{}/invites", room_path))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("user_id=3"))
        .unwrap();
    let (status, _, location, _) = request(app.clone(), invite_req).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(room_path.as_str()));

    // Carol login
    let login_carol = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=carol%40example.com&password=carol-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_carol).await;
    let carol_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Carol sees the invite on her chat page
    let (status, body, _, _) = request(
        app.clone(),
        Request::builder()
            .uri("/chat")
            .header(header::COOKIE, &carol_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Secret"));
    assert!(html.contains("Invited by Alice"));

    // Carol accepts the invite (invite ID should be 1 as it's the first one in the DB)
    let accept_req = Request::builder()
        .method("POST")
        .uri("/chat/invites/1/accept")
        .header(header::COOKIE, &carol_cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, location, _) = request(app.clone(), accept_req).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(room_path.as_str()));

    // Carol can now see the room content
    let (status, body, _, _) = request(
        app.clone(),
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &carol_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body).unwrap().contains("Secret"));
}

#[tokio::test]
async fn invite_validation_errors() {
    let app = test_app().await;

    // Alice login
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice%40example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Create room
    let (_, _, location, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/rooms")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("name=Secret&participant_ids=2"))
            .unwrap(),
    )
    .await;
    let room_path = location.unwrap();

    // Invite self
    let (status, body, _, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("{}/invites", room_path))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("user_id=1"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("You cannot invite yourself."));

    // Invite Bob (already member)
    let (status, body, _, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("{}/invites", room_path))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("user_id=2"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("That user is already part of this chat."));

    // Invite to General (ID 1 is General room)
    let (status, body, _, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/rooms/1/invites")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("user_id=2"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("The general chat cannot be restricted by invitation."));
}

#[tokio::test]
async fn serve_file_returns_not_found_for_missing_file() {
    let app = test_app().await;
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice%40example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    let (status, _, _, _) = request(
        app,
        Request::builder()
            .uri("/chat/files/999")
            .header(header::COOKIE, alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn password_length_validation_on_signup() {
    let app = test_app().await;
    let (status, body, _) = post_form(
        app,
        "/users",
        "name=Dave&email=dave%40example.com&password=short",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Password must be at least 8 characters."));
}

#[tokio::test]
async fn accept_invite_validation_errors() {
    let app = test_app().await;

    // Alice login
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice%40example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Alice creates a private room with Bob (Bob is ID 2)
    let (_, _, location, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/rooms")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("name=Secret&participant_ids=2"))
            .unwrap(),
    )
    .await;
    let room_path = location.unwrap();

    // Alice invites Carol (Carol will be 3)
    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;
    request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri(format!("{}/invites", room_path))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("user_id=3"))
            .unwrap(),
    )
    .await;

    // Bob tries to accept Carol's invite (invite ID 1)
    let login_bob = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=bob%40example.com&password=bob-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_bob).await;
    let bob_cookie = cookie_value(set_cookie.as_deref().unwrap());

    let (status, _, _, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/invites/1/accept")
            .header(header::COOKIE, &bob_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND); // Mismatched user
}

#[tokio::test]
async fn serve_file_success() {
    let (app, pool) = test_app_with_pool().await;

    // Insert a message with a file manually
    sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, file_name, file_data, file_content_type)
         VALUES (1, 1, 'Here is a file', 'user', 'test.txt', ?, 'text/plain')"
    )
    .bind("Hello world".as_bytes())
    .execute(&pool)
    .await
    .expect("insert test file message");

    // Alice login
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice%40example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Request the file (message ID 1)
    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri("/chat/files/1")
            .header(header::COOKIE, alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "Hello world".as_bytes());
}

#[tokio::test]
async fn cannot_access_private_room_without_invite() {
    let app = test_app().await;

    // Alice login
    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=alice%40example.com&password=alice-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Alice creates a private room with Bob
    let (_, _, location, _) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/chat/rooms")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &alice_cookie)
            .body(Body::from("name=Secret&participant_ids=2"))
            .unwrap(),
    )
    .await;
    let room_path = location.unwrap();

    // Carol login (Carol is not in the room)
    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;
    let login_carol = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=carol%40example.com&password=carol-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_carol).await;
    let carol_cookie = cookie_value(set_cookie.as_deref().unwrap());

    // Carol tries to access Alice's secret room
    let (status, _, location, _) = request(
        app.clone(),
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &carol_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/chat"));
}
