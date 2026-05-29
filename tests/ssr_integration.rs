mod common;

use axum::http::StatusCode;
use common::{get, post_form, request, test_app};
use axum::body::Body;
use axum::http::{header, Request};

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

    let cookie = common::cookie_value(set_cookie.as_deref().expect("set cookie"));
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
