mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::{cookie_value, post_form, request, test_app, test_app_with_pool};
use serde_json::Value;

#[tokio::test]
async fn one_to_one_room_shows_e2e_badge() {
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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_req).await;
    let room_path = location.unwrap();

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("E2E Encrypted"));
}

#[tokio::test]
async fn multi_person_room_no_e2e_badge() {
    let app = test_app().await;

    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;

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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Group&participant_ids=2,3"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_req).await;
    let room_path = location.unwrap();

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(!html.contains("E2E Encrypted"));
    assert!(html.contains("Private"));
}

#[tokio::test]
async fn crypto_public_key_store_and_retrieve() {
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

    let store_req = Request::builder()
        .method("POST")
        .uri("/api/crypto/public-key")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from(
            r#"{"public_key":"{\"kty\":\"RSA\",\"n\":\"test\"}"}"#,
        ))
        .unwrap();
    let (status, _, _, _) = request(app.clone(), store_req).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri("/api/crypto/public-key/1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        resp["public_key"].as_str().unwrap(),
        r#"{"kty":"RSA","n":"test"}"#
    );
}

#[tokio::test]
async fn crypto_room_key_store_and_retrieve() {
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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, _, _) = request(app.clone(), create_req).await;

    let store_req = Request::builder()
        .method("POST")
        .uri("/api/crypto/room-key/2")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from(
            r#"{"user_id":2,"encrypted_key":"wrapped-key-123"}"#,
        ))
        .unwrap();
    let (status, _, _, _) = request(app.clone(), store_req).await;
    assert_eq!(status, StatusCode::OK);

    let login_bob = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("email=bob@example.com&password=bob-password"))
        .unwrap();
    let (_, _, _, set_cookie) = request(app.clone(), login_bob).await;
    let bob_cookie = cookie_value(set_cookie.as_deref().unwrap());

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri("/api/crypto/room-key/2")
            .header(header::COOKIE, &bob_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp["encrypted_key"].as_str().unwrap(), "wrapped-key-123");
}

#[tokio::test]
async fn crypto_room_key_rejects_non_member_target() {
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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, _, _) = request(app.clone(), create_req).await;

    let store_req = Request::builder()
        .method("POST")
        .uri("/api/crypto/room-key/2")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from(
            r#"{"user_id":999,"encrypted_key":"wrapped-key-123"}"#,
        ))
        .unwrap();
    let (status, _, _, _) = request(app, store_req).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn encrypted_message_renders_placeholder_on_room_page() {
    let (app, pool) = test_app_with_pool().await;

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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_req).await;
    let room_path = location.unwrap();

    sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, is_encrypted) VALUES (2, 1, 'cipher-text-here', 'user', 1)",
    )
    .execute(&pool)
    .await
    .expect("insert encrypted message");

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("[Encrypted]</p>"));
    assert!(html.contains("const ROOM_KEY_OWNER_ID = 1;"));
}

#[tokio::test]
async fn multi_person_room_message_is_not_encrypted() {
    let (app, pool) = test_app_with_pool().await;

    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;

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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Group&participant_ids=2,3"))
        .unwrap();
    let (_, _, location, _) = request(app.clone(), create_req).await;
    let room_path = location.unwrap();

    sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, is_encrypted) VALUES (2, 1, 'hello group', 'user', 0)",
    )
    .execute(&pool)
    .await
    .expect("insert message");

    let (status, body, _, _) = request(
        app,
        Request::builder()
            .uri(&room_path)
            .header(header::COOKIE, &alice_cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("hello group</p>"));
    assert!(!html.contains("[Encrypted]</p>"));
}

#[tokio::test]
async fn chat_service_one_to_one_is_encrypted() {
    let (app, pool) = test_app_with_pool().await;
    let cache = rust_api_ssr::cache::AppCache::new();
    let chat_service = rust_api_ssr::services::chat::ChatService::new(pool, cache);
    let ctx = rust_api_ssr::context::QueryContext {
        bypass_cache: false,
    };

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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Secret&participant_ids=2"))
        .unwrap();
    let (_, _, _, _) = request(app, create_req).await;

    let room = chat_service
        .get_room_for_user(ctx, 1, 2)
        .await
        .unwrap()
        .unwrap();
    assert!(room.is_encrypted);
}

#[tokio::test]
async fn chat_service_multi_person_is_not_encrypted() {
    let (app, pool) = test_app_with_pool().await;

    post_form(
        app.clone(),
        "/users",
        "name=Carol&email=carol%40example.com&password=carol-password",
    )
    .await;

    let cache = rust_api_ssr::cache::AppCache::new();
    let chat_service = rust_api_ssr::services::chat::ChatService::new(pool, cache);
    let ctx = rust_api_ssr::context::QueryContext {
        bypass_cache: false,
    };

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

    let create_req = Request::builder()
        .method("POST")
        .uri("/chat/rooms")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &alice_cookie)
        .body(Body::from("name=Group&participant_ids=2,3"))
        .unwrap();
    let (_, _, _, _) = request(app, create_req).await;

    let room = chat_service
        .get_room_for_user(ctx, 1, 2)
        .await
        .unwrap()
        .unwrap();
    assert!(!room.is_encrypted);
}
