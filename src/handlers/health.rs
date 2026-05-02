use crate::error::AppError;
use crate::handlers::AppState;
use axum::{extract::State, http::StatusCode};

pub async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub async fn readiness(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    state.user_service().list_users().await?;
    Ok(StatusCode::NO_CONTENT)
}
