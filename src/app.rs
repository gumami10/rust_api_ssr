use crate::handlers::{api, health, views, AppState};
use crate::middleware::log_request_latency;
use axum::{
    middleware,
    routing::{get, post},
    Router,
};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(views::render_index))
        .route("/healthz", get(health::health))
        .route("/readyz", get(health::readiness))
        .route("/users", get(views::render_index).post(views::create_user))
        .route("/users/new", get(views::render_new_user))
        .route(
            "/users/:id",
            get(views::render_user).post(views::update_user),
        )
        .route("/users/:id/edit", get(views::render_edit_user))
        .route("/users/:id/delete", post(views::delete_user))
        .route("/login", get(crate::handlers::auth::render_login).post(crate::handlers::auth::login))
        .route("/logout", post(crate::handlers::auth::logout))
        .route("/chat", get(crate::handlers::chat::render_chat))
        .route("/chat/rooms", post(crate::handlers::chat::create_chat_room))
        .route("/chat/rooms/:id", get(crate::handlers::chat::render_chat_room))
        .route(
            "/chat/rooms/:id/invites",
            post(crate::handlers::chat::invite_to_room),
        )
        .route(
            "/chat/invites/:id/accept",
            post(crate::handlers::chat::accept_invite),
        )
        .route("/chat/ws", get(crate::handlers::chat::chat_ws))
        .route(
            "/chat/files/:id",
            get(crate::handlers::chat::serve_file),
        )
        .route(
            "/api/crypto/public-key/:user_id",
            get(crate::handlers::chat::get_public_key_handler),
        )
        .route(
            "/api/crypto/public-key",
            post(crate::handlers::chat::store_public_key_handler),
        )
        .route(
            "/api/crypto/room-key/:room_id",
            get(crate::handlers::chat::get_room_key_handler)
                .post(crate::handlers::chat::store_room_key_handler),
        )
        .route(
            "/api/crypto/room-key/:room_id/members",
            get(crate::handlers::chat::get_room_key_members_handler),
        )
        .route("/api/users", get(api::list_users))
        .route("/api/users/:id", get(api::get_user).delete(api::delete_user))
        .route("/perf", get(views::render_perf))
        .route("/encryption", get(views::render_encryption))
        .route("/about", get(views::render_about))
        .layer(middleware::from_fn_with_state(state.clone(), log_request_latency))
        .with_state(state)
}
