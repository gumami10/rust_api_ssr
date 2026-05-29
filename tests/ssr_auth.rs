mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::{cookie_value, csrf_token_from_cookie, get, post_form, request, test_app};

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
async fn login_form_validation_errors() {
    let app = test_app().await;

    let (status, body, _) = post_form(app.clone(), "/login", "email=&password=pass").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Email is required."));

    let (status, body, _) = post_form(app.clone(), "/login", "email=invalid&password=pass").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Email must contain @."));

    let (status, body, _) =
        post_form(app.clone(), "/login", "email=alice@example.com&password=").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Password is required."));

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

    let login_req = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_req).await;
    let cookie = cookie_value(set_cookie.as_deref().unwrap());
    let csrf = csrf_token_from_cookie(set_cookie.as_deref().unwrap()).unwrap();

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

    let full_cookie = format!("{}; csrf_token={}", cookie, csrf);
    let (status, _, location, set_cookie) = request(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/logout")
            .header(header::COOKIE, &full_cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("csrf_token={}", csrf)))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/users"));
    assert!(set_cookie.as_deref().unwrap().contains("Max-Age=0"));

    let (status, _, _, _) = request(
        app.clone(),
        Request::builder()
            .uri("/chat")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}
