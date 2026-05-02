use crate::error::AppError;
use crate::handlers::AppState;
use crate::models::user::User;
use axum::{
    extract::{Path, State},
    Json,
};

pub async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>, AppError> {
    let user = state
        .user_repo
        .get_user_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", id)))?;

    Ok(Json(user))
}

pub async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<User>>, AppError> {
    let users = state.user_repo.list_users().await?;
    Ok(Json(users))
}
