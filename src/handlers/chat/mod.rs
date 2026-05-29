mod crypto;
mod messages;
mod rooms;
mod ws;

pub use crypto::{
    delete_device_handler, get_public_key_handler, get_room_key_handler,
    get_room_key_members_handler, list_devices_handler, register_device_handler,
    store_public_key_handler, store_room_key_handler, DeviceResponse, DevicesResponse,
    PublicDeviceKey, PublicKeyResponse, RegisterDeviceInput, RoomDeviceKeyResponse,
    RoomKeyMembersResponse, RoomKeyResponse, StorePublicKeyInput, StoreRoomKeyInput,
};
pub use messages::serve_file;
pub use rooms::{accept_invite, create_chat_room, invite_to_room, render_chat, render_chat_room};
pub use ws::chat_ws;

use crate::error::AppError;
use crate::handlers::RequestMetric;
use crate::models::user::User;
use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub const GENERAL_ROOM_ID: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChatEvent {
    pub id: i64,
    pub room_id: i64,
    pub user_name: String,
    pub body: String,
    pub created_at: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub file_name: Option<String>,
    pub file_content_type: Option<String>,
    #[serde(default)]
    pub is_encrypted: bool,
}

fn default_kind() -> String {
    "user".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BroadcastEvent {
    #[serde(rename = "message")]
    Message(ChatEvent),
    #[serde(rename = "typing")]
    Typing {
        room_id: i64,
        user_name: String,
        is_typing: bool,
    },
}

#[derive(Debug, Clone, FromRow)]
pub struct ChatRoomRow {
    pub id: i64,
    pub name: String,
    pub is_general: bool,
    pub created_by_user_id: Option<i64>,
    pub participant_count: i64,
}

#[derive(Debug, Clone)]
pub struct ChatRoomView {
    pub id: i64,
    pub name: String,
    pub is_general: bool,
    pub created_by_user_id: Option<i64>,
    pub participant_count: i64,
    pub path: String,
    pub is_active: bool,
    pub unread_count: i64,
    pub is_encrypted: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatParticipant {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct PendingInviteRow {
    pub id: i64,
    pub room_id: i64,
    pub room_name: String,
    pub invited_by_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PendingInviteView {
    pub room_name: String,
    pub invited_by_name: String,
    pub created_at: String,
    pub accept_path: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomForm {
    pub name: Option<String>,
    pub participant_ids: String,
}

#[derive(Debug, Deserialize)]
pub struct InviteForm {
    pub user_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct RoomQuery {
    pub room_id: Option<i64>,
}

#[derive(Template)]
#[template(path = "chat/index.html")]
pub struct ChatTemplate {
    pub viewer: Option<User>,
    pub request_metrics: Vec<RequestMetric>,
    pub user: User,
    pub room: ChatRoomView,
    pub rooms: Vec<ChatRoomView>,
    pub messages: Vec<ChatEvent>,
    pub participants: Vec<ChatParticipant>,
    pub participants_json: String,
    pub all_users: Vec<User>,
    pub available_invitees: Vec<User>,
    pub pending_invites: Vec<PendingInviteView>,
    pub error: Option<String>,
}

pub fn room_path(room_id: i64, is_general: bool) -> String {
    if is_general || room_id == GENERAL_ROOM_ID {
        "/chat".to_string()
    } else {
        format!("/chat/rooms/{}", room_id)
    }
}

pub fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
