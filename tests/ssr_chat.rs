mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::{cookie_value, post_form, request, test_app, test_app_with_pool};

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
    let (status, _) = common::get(test_app().await, "/chat").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn create_room_validation_errors() {
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

    assert_eq!(status, StatusCode::OK);
    assert_eq!(location, None);
    assert!(String::from_utf8(body)
        .unwrap()
        .contains("Add at least one other participant"));
}

#[tokio::test]
async fn invite_and_accept_flow() {
    let app = test_app().await;

    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

    let create_room = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_room).await;
    let room_path = location.unwrap();

    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;

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

    let login_carol = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=carol%40example.com&password=carol-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_carol).await;
    let carol_cookie = cookie_value(set_cookie.as_deref().unwrap());

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

    let accept_req = Request::builder()
        .method("POST")
        .uri("/chat/invites/1/accept")
        .header(header::COOKIE, &carol_cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, location, _) = request(app.clone(), accept_req).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some(room_path.as_str()));

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

    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

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
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
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
async fn accept_invite_validation_errors() {
    let app = test_app().await;

    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

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

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serve_file_success() {
    let (app, pool) = test_app_with_pool().await;

    sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, file_name, file_data, file_content_type)
         VALUES (1, 1, 'Here is a file', 'user', 'test.txt', ?, 'text/plain')"
    )
    .bind("Hello world".as_bytes())
    .execute(&pool)
    .await
    .expect("insert test file message");

    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

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

    let login_alice = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "email=alice@example.com&password=alice-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_alice).await;
    let alice_cookie = cookie_value(set_cookie.as_deref().unwrap());

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
        .body(Body::from(
            "email=carol%40example.com&password=carol-password",
        ))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_carol).await;
    let carol_cookie = cookie_value(set_cookie.as_deref().unwrap());

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
