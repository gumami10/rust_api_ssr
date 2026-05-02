use crate::handlers::{api, views, AppState};
use crate::middleware::log_request_latency;
use axum::{middleware, routing::get, Router};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(views::render_index))
        .route("/api/users", get(api::list_users))
        .route("/api/users/:id", get(api::get_user))
        .layer(middleware::from_fn(log_request_latency))
        .with_state(state)
}
