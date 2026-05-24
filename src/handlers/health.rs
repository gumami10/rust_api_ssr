use crate::error::AppError;
use crate::handlers::AppState;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
};

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store, must-revalidate")],
    )
}

pub async fn readiness(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    state
        .user_service
        .list_users(crate::context::QueryContext::default())
        .await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store, must-revalidate")],
    ))
}
