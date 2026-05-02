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
        .route("/chat/ws", get(crate::handlers::chat::chat_ws))
        .route("/api/users", get(api::list_users))
        .route("/api/users/:id", get(api::get_user).delete(api::delete_user))
        .layer(middleware::from_fn(log_request_latency))
        .with_state(state)
}
