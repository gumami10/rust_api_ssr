use crate::services::users::UserServiceError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tracing::{debug, error};

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal server error")]
    Internal,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AppError::Database(err) => {
                error!("Database error: {:?}", err);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            AppError::Conflict(msg) => {
                debug!("Conflict: {}", msg);
                (StatusCode::CONFLICT, msg.as_str())
            }
            AppError::NotFound(msg) => {
                debug!("Not found: {}", msg);
                (StatusCode::NOT_FOUND, msg.as_str())
            }
            AppError::Internal => {
                error!("Internal server error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };

        let body = Json(json!({
            "error": error_message
        }));

        (status, body).into_response()
    }
}

impl From<UserServiceError> for AppError {
    fn from(err: UserServiceError) -> Self {
        match err {
            UserServiceError::Database(err) => AppError::Database(err),
            UserServiceError::DuplicateEmail => {
                AppError::Conflict("Email already exists.".to_string())
            }
            UserServiceError::InvalidCredentials | UserServiceError::PasswordHash(_) => {
                AppError::Internal
            }
        }
    }
}
