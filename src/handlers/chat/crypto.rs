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

#[derive(Debug, Deserialize)]
pub struct RegisterDeviceInput {
    pub device_id: String,
    pub device_name: Option<String>,
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct PublicDeviceKey {
    pub device_id: String,
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub device_id: String,
    pub device_name: Option<String>,
    pub public_key: String,
    pub created_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Serialize)]
pub struct DevicesResponse {
    pub devices: Vec<DeviceResponse>,
}

#[derive(Debug, Serialize)]
pub struct PublicKeyResponse {
    pub public_key: Option<String>,
    pub devices: Vec<PublicDeviceKey>,
}

#[derive(Debug, Deserialize)]
pub struct StoreRoomKeyInput {
    pub user_id: i64,
    pub device_id: Option<String>,
    pub encrypted_key: String,
}

#[derive(Debug, Serialize)]
pub struct RoomDeviceKeyResponse {
    pub device_id: String,
    pub encrypted_key: String,
}

#[derive(Debug, Serialize)]
pub struct RoomKeyResponse {
    pub encrypted_key: Option<String>,
    pub keys: Vec<RoomDeviceKeyResponse>,
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
    let devices = state.chat_service.get_user_devices(ctx, user_id).await?;
    let public_key = devices.first().map(|device| device.public_key.clone());
    let devices = devices
        .into_iter()
        .map(|device| PublicDeviceKey {
            device_id: device.device_id,
            public_key: device.public_key,
        })
        .collect();
    let body = serde_json::to_string(&PublicKeyResponse {
        public_key,
        devices,
    })
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

pub async fn register_device_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterDeviceInput>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    if input.device_id.trim().is_empty() || input.public_key.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Device id and public key are required",
        )
            .into_response());
    }
    state
        .chat_service
        .register_device(
            user.id,
            &input.device_id,
            input.device_name.as_deref(),
            &input.public_key,
        )
        .await?;
    Ok((StatusCode::OK, "OK").into_response())
}

pub async fn list_devices_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    let devices = state.chat_service.get_user_devices(ctx, user.id).await?;
    let devices = devices
        .into_iter()
        .map(|device| DeviceResponse {
            device_id: device.device_id,
            device_name: device.device_name,
            public_key: device.public_key,
            created_at: device.created_at,
            last_seen_at: device.last_seen_at,
        })
        .collect();
    let body =
        serde_json::to_string(&DevicesResponse { devices }).map_err(|_| AppError::Internal)?;
    Ok((StatusCode::OK, HeaderMap::new(), body).into_response())
}

pub async fn delete_device_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
    };
    state
        .chat_service
        .delete_device(user.id, &device_id)
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
    let keys = state
        .chat_service
        .get_encrypted_room_device_keys(ctx, room_id, user.id)
        .await?;
    let encrypted_key = keys.first().map(|key| key.encrypted_key.clone());
    let keys = keys
        .into_iter()
        .map(|key| RoomDeviceKeyResponse {
            device_id: key.device_id,
            encrypted_key: key.encrypted_key,
        })
        .collect();
    let body = serde_json::to_string(&RoomKeyResponse {
        encrypted_key,
        keys,
    })
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
    if let Some(device_id) = input.device_id.as_deref() {
        if !state
            .chat_service
            .device_belongs_to_user(input.user_id, device_id)
            .await?
        {
            return Ok((StatusCode::FORBIDDEN, "Target device is not registered").into_response());
        }
        state
            .chat_service
            .store_encrypted_room_device_key(
                room_id,
                input.user_id,
                device_id,
                &input.encrypted_key,
            )
            .await?;
    } else {
        state
            .chat_service
            .store_encrypted_room_key(room_id, input.user_id, &input.encrypted_key)
            .await?;
    }
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
