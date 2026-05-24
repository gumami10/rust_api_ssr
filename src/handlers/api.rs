use crate::error::AppError;
use crate::handlers::AppState;
use crate::handlers::query_context;
use crate::models::user::User;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};

pub async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<User>, AppError> {
    let ctx = query_context(&headers);
    let user = state
        .user_service
        .get_user(ctx, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", id)))?;

    Ok(Json(user))
}

pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<User>>, AppError> {
    let ctx = query_context(&headers);
    let users = state.user_service.list_users(ctx).await?;
    Ok(Json(users))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let deleted = state.user_service.delete_user(id).await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!("User with id {} not found", id)))
    }
}
