use crate::error::AppError;
use crate::handlers::{auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct StorePublicKeyInput {
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct PublicKeyResponse {
    pub public_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StoreRoomKeyInput {
    pub user_id: i64,
    pub encrypted_key: String,
}

#[derive(Debug, Serialize)]
pub struct RoomKeyResponse {
    pub encrypted_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RoomKeyMembersResponse {
    pub member_ids: Vec<i64>,
}

pub async fn get_public_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let key = state.chat_service.get_public_key(ctx, user_id).await?;
    let body = serde_json::to_string(&PublicKeyResponse { public_key: key })
        .map_err(|_| AppError::Internal)?;
    Ok((StatusCode::OK, HeaderMap::new(), body).into_response())
}

pub async fn store_public_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StorePublicKeyInput>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    state
        .chat_service
        .store_public_key(user.id, &input.public_key)
        .await?;
    Ok((StatusCode::OK, "OK").into_response())
}

pub async fn get_room_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Ok((StatusCode::FORBIDDEN, "Forbidden").into_response());
    };
    if !room.is_encrypted {
        return Ok((StatusCode::BAD_REQUEST, "Room does not use E2E keys").into_response());
    }
    let key = state
        .chat_service
        .get_encrypted_room_key(ctx, room_id, user.id)
        .await?;
    let body = serde_json::to_string(&RoomKeyResponse { encrypted_key: key })
        .map_err(|_| AppError::Internal)?;
    Ok((StatusCode::OK, HeaderMap::new(), body).into_response())
}

pub async fn store_room_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
    Json(input): Json<StoreRoomKeyInput>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Ok((StatusCode::FORBIDDEN, "Forbidden").into_response());
    };
    if !room.is_encrypted {
        return Ok((StatusCode::BAD_REQUEST, "Room does not use E2E keys").into_response());
    }
    if !state
        .chat_service
        .is_room_member(ctx, room_id, input.user_id)
        .await?
    {
        return Ok((StatusCode::FORBIDDEN, "Target user is not a room member").into_response());
    }
    state
        .chat_service
        .store_encrypted_room_key(room_id, input.user_id, &input.encrypted_key)
        .await?;
    Ok((StatusCode::OK, "OK").into_response())
}

pub async fn get_room_key_members_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Ok((StatusCode::FORBIDDEN, "Forbidden").into_response());
    };
    if !room.is_encrypted {
        return Ok((StatusCode::BAD_REQUEST, "Room does not use E2E keys").into_response());
    }
    let member_ids = state
        .chat_service
        .get_room_key_member_ids(ctx, room_id)
        .await?;
    let body = serde_json::to_string(&RoomKeyMembersResponse { member_ids })
        .map_err(|_| AppError::Internal)?;
    Ok((StatusCode::OK, HeaderMap::new(), body).into_response())
}
