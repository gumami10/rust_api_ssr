use crate::error::AppError;
use crate::handlers::{auth, AppState, RequestMetric};
use crate::models::user::User;
use askama::Template;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Form, Path, Query, State,
    },
    http::HeaderMap,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashSet;
use tokio::sync::broadcast;

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

/// Envelope sent over the broadcast channel and serialized to WS clients.
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
    name: Option<String>,
    participant_ids: String,
}

#[derive(Debug, Deserialize)]
pub struct InviteForm {
    user_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct RoomQuery {
    room_id: Option<i64>,
}

#[derive(Template)]
#[template(path = "chat/index.html")]
struct ChatTemplate {
    viewer: Option<User>,
    request_metrics: Vec<RequestMetric>,
    user: User,
    room: ChatRoomView,
    rooms: Vec<ChatRoomView>,
    messages: Vec<ChatEvent>,
    participants: Vec<ChatParticipant>,
    participants_json: String,
    all_users: Vec<User>,
    available_invitees: Vec<User>,
    pending_invites: Vec<PendingInviteView>,
    error: Option<String>,
}

pub async fn render_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    render_chat_room_page(&state, user, GENERAL_ROOM_ID, None, ctx).await
}

pub async fn render_chat_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    render_chat_room_page(&state, user, room_id, None, ctx).await
}

pub async fn create_chat_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CreateRoomForm>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let mut participant_ids = form
        .participant_ids
        .split(',')
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .collect::<Vec<_>>();
    participant_ids.retain(|id| *id != user.id);
    participant_ids.sort_unstable();
    participant_ids.dedup();

    if participant_ids.is_empty() {
        return render_chat_room_page(
            &state,
            user,
            GENERAL_ROOM_ID,
            Some("Add at least one other participant to create a private chat.".to_string()),
            ctx,
        )
        .await;
    }

    let selected_users = state
        .user_service
        .get_users_by_ids(ctx, &participant_ids, &state.pool)
        .await?;
    if selected_users.len() != participant_ids.len() {
        return render_chat_room_page(
            &state,
            user,
            GENERAL_ROOM_ID,
            Some("One or more selected users could not be found.".to_string()),
            ctx,
        )
        .await;
    }

    let room_name = form.name.unwrap_or_default().trim().to_string();
    let room_name = if room_name.is_empty() {
        default_room_name(&selected_users)
    } else {
        room_name
    };

    let mut tx = state.pool.begin().await.map_err(AppError::Database)?;

    let result = sqlx::query(
        r#"
        INSERT INTO chat_rooms (name, kind, created_by_user_id)
        VALUES (?, 'private', ?)
        "#,
    )
    .bind(&room_name)
    .bind(user.id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    let room_id = result.last_insert_rowid();

    sqlx::query("INSERT INTO chat_room_members (room_id, user_id) VALUES (?, ?)")
        .bind(room_id)
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    for participant_id in &participant_ids {
        sqlx::query("INSERT INTO chat_room_members (room_id, user_id) VALUES (?, ?)")
            .bind(room_id)
            .bind(*participant_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
    }

    tx.commit().await.map_err(AppError::Database)?;

    // Invalidate caches for all participants + creator
    for pid in &participant_ids {
        state.chat_service.invalidate_chat_for_user(*pid).await;
        state
            .chat_service
            .invalidate_accessible_room(*pid, room_id)
            .await;
    }
    state.chat_service.invalidate_chat_for_user(user.id).await;
    state
        .chat_service
        .invalidate_accessible_room(user.id, room_id)
        .await;
    state.chat_service.invalidate_chat_for_room(room_id).await;

    // Emit notification
    if let Ok(event) = persist_notification(
        &state,
        &user,
        room_id,
        &format!("{} created this room", user.name),
    )
    .await
    {
        let _ = state.chat_tx.send(BroadcastEvent::Message(event));
    }

    Ok(Redirect::to(&room_path(room_id, false)).into_response())
}

pub async fn invite_to_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
    Form(form): Form<InviteForm>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Ok(Redirect::to("/chat").into_response());
    };

    if room.is_general {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("The general chat cannot be restricted by invitation.".to_string()),
            ctx,
        )
        .await;
    }

    let invited_user = state
        .user_service
        .get_user(ctx, form.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", form.user_id)))?;

    if invited_user.id == user.id {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("You cannot invite yourself.".to_string()),
            ctx,
        )
        .await;
    }

    let participants = state
        .chat_service
        .get_room_participants(ctx, room.id)
        .await?;
    if participants
        .iter()
        .any(|participant| participant.id == invited_user.id)
    {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("That user is already part of this chat.".to_string()),
            ctx,
        )
        .await;
    }

    let pending_invite = sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT id
        FROM chat_room_invites
        WHERE room_id = ? AND invited_user_id = ? AND status = 'pending'
        "#,
    )
    .bind(room.id)
    .bind(invited_user.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(AppError::Database)?;

    if pending_invite.is_some() {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("That user already has a pending invitation.".to_string()),
            ctx,
        )
        .await;
    }

    let member_ids: HashSet<i64> = participants
        .into_iter()
        .map(|participant| participant.id)
        .collect();
    if !member_ids.contains(&user.id) {
        return Ok(Redirect::to("/chat").into_response());
    }

    sqlx::query(
        r#"
        INSERT INTO chat_room_invites (room_id, invited_user_id, invited_by_user_id)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(room.id)
    .bind(invited_user.id)
    .bind(user.id)
    .execute(&state.pool)
    .await
    .map_err(AppError::Database)?;

    state
        .chat_service
        .invalidate_pending_invites(invited_user.id)
        .await;

    Ok(Redirect::to(&room.path).into_response())
}

pub async fn accept_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let invite = sqlx::query_as::<_, PendingInviteRow>(
        r#"
        SELECT invites.id, invites.room_id, rooms.name AS room_name, inviter.name AS invited_by_name, invites.created_at
        FROM chat_room_invites invites
        INNER JOIN chat_rooms rooms ON rooms.id = invites.room_id
        INNER JOIN users inviter ON inviter.id = invites.invited_by_user_id
        WHERE invites.id = ? AND invites.invited_user_id = ? AND invites.status = 'pending'
        "#,
    )
    .bind(invite_id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let Some(invite) = invite else {
        return Err(AppError::NotFound(format!(
            "Invite with id {} not found",
            invite_id
        )));
    };

    let mut tx = state.pool.begin().await.map_err(AppError::Database)?;
    sqlx::query("INSERT OR IGNORE INTO chat_room_members (room_id, user_id) VALUES (?, ?)")
        .bind(invite.room_id)
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    sqlx::query(
        r#"
        UPDATE chat_room_invites
        SET status = 'accepted', accepted_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(invite.id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    state.chat_service.invalidate_chat_for_user(user.id).await;
    state
        .chat_service
        .invalidate_accessible_room(user.id, invite.room_id)
        .await;
    state
        .chat_service
        .invalidate_chat_for_room(invite.room_id)
        .await;
    state.chat_service.invalidate_all_unread_counts().await;

    // Emit notification
    if let Ok(event) = persist_notification(
        &state,
        &user,
        invite.room_id,
        &format!("{} joined the room", user.name),
    )
    .await
    {
        let _ = state.chat_tx.send(BroadcastEvent::Message(event));
    }

    Ok(Redirect::to(&room_path(invite.room_id, false)).into_response())
}

pub async fn chat_ws(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RoomQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let room_id = query.room_id.unwrap_or(GENERAL_ROOM_ID);
    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Ok(Redirect::to("/chat").into_response());
    };

    let is_encrypted = room.is_encrypted;
    Ok(ws
        .on_upgrade(move |socket| handle_socket(socket, state, user, room.id, is_encrypted))
        .into_response())
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    user: User,
    room_id: i64,
    is_encrypted: bool,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut broadcast_rx = state.chat_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(payload))) => {
                        if let Ok(input) = serde_json::from_str::<ChatInput>(payload.as_str()) {
                            // Handle typing indicator
                            if let Some(is_typing) = input.typing {
                                let _ = state.chat_tx.send(BroadcastEvent::Typing {
                                    room_id,
                                    user_name: user.name.clone(),
                                    is_typing,
                                });
                                continue;
                            }

                            let body = input.body.as_deref().unwrap_or("").trim().to_string();

                            // Handle file attachment
                            if let (Some(ref file_b64), Some(ref file_name)) = (&input.file_data, &input.file_name) {
                                let content_type = input.file_content_type.clone().unwrap_or_else(|| "application/octet-stream".to_string());
                                if let Ok(file_bytes) = base64::engine::general_purpose::STANDARD.decode(file_b64) {
                                    let display_body = if body.is_empty() {
                                        format!("shared a file: {}", file_name)
                                    } else {
                                        body.clone()
                                    };
                                    if let Ok(event) = persist_message_with_file(
                                        &state, &user, room_id, &display_body,
                                        file_name, &file_bytes, &content_type, is_encrypted,
                                    ).await {
                                        let _ = state.chat_tx.send(BroadcastEvent::Message(event));
                                    }
                                }
                                continue;
                            }

                            if body.is_empty() {
                                continue;
                            }

                            if let Ok(event) = persist_message(&state, &user, room_id, &body, is_encrypted).await {
                                let _ = state.chat_tx.send(BroadcastEvent::Message(event));
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            broadcasted = broadcast_rx.recv() => {
                let should_forward = match &broadcasted {
                    Ok(BroadcastEvent::Message(event)) => event.room_id == room_id,
                    Ok(BroadcastEvent::Typing { room_id: rid, .. }) => *rid == room_id,
                    Err(_) => false,
                };
                match broadcasted {
                    Ok(event) if should_forward => {
                        let payload = serde_json::to_string(&event).unwrap_or_default();
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn render_chat_room_page(
    state: &AppState,
    viewer: User,
    room_id: i64,
    error: Option<String>,
    ctx: crate::context::QueryContext,
) -> Result<Response, AppError> {
    let Some(room) = state
        .chat_service
        .get_room_for_user(ctx, viewer.id, room_id)
        .await?
    else {
        return Ok(Redirect::to("/chat").into_response());
    };

    // Mark current room as read
    let _ = update_read_position(state, viewer.id, room.id).await;

    let request_metrics = state.request_metrics.recent();
    let messages = state.chat_service.get_chat_messages(ctx, room.id).await?;
    let participants = state
        .chat_service
        .get_room_participants(ctx, room.id)
        .await?;
    let all_users = state
        .user_service
        .list_users(ctx)
        .await?
        .into_iter()
        .filter(|candidate| candidate.id != viewer.id)
        .collect::<Vec<_>>();
    let pending_invites = state
        .chat_service
        .get_pending_invites(ctx, viewer.id)
        .await?;
    let unread_counts = state.chat_service.get_unread_counts(ctx, viewer.id).await?;

    let active_room_id = room.id;
    let mut rooms = state
        .chat_service
        .get_accessible_rooms(ctx, viewer.id)
        .await?;
    for room in &mut rooms {
        room.is_active = room.id == active_room_id;
        room.unread_count = if room.id == active_room_id {
            0
        } else {
            unread_counts.get(&room.id).copied().unwrap_or(0)
        };
    }

    let available_invitees = available_invitees(&all_users, viewer.id, &participants);
    let participants_json =
        serde_json::to_string(&participants).unwrap_or_else(|_| "[]".to_string());

    render_template(
        ChatTemplate {
            viewer: Some(viewer.clone()),
            request_metrics,
            user: viewer,
            room,
            rooms,
            messages,
            participants,
            participants_json,
            all_users,
            available_invitees,
            pending_invites,
            error,
        },
        StatusCode::OK,
    )
}

async fn persist_message(
    state: &AppState,
    user: &User,
    room_id: i64,
    body: &str,
    is_encrypted: bool,
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query("INSERT INTO chat_messages (room_id, user_id, body, kind, is_encrypted) VALUES (?, ?, ?, 'user', ?)")
        .bind(room_id)
        .bind(user.id)
        .bind(body)
        .bind(if is_encrypted { 1 } else { 0 })
        .execute(&state.pool)
        .await
        .map_err(AppError::Database)?;

    let id = result.last_insert_rowid();
    let created_at: (String,) = sqlx::query_as("SELECT created_at FROM chat_messages WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(AppError::Database)?;

    state.chat_service.invalidate_chat_for_room(room_id).await;
    state.chat_service.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "user".to_string(),
        file_name: None,
        file_content_type: None,
        is_encrypted,
    })
}

async fn persist_message_with_file(
    state: &AppState,
    user: &User,
    room_id: i64,
    body: &str,
    file_name: &str,
    file_data: &[u8],
    content_type: &str,
    is_encrypted: bool,
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, is_encrypted, file_name, file_data, file_content_type) VALUES (?, ?, ?, 'user', ?, ?, ?, ?)",
    )
    .bind(room_id)
    .bind(user.id)
    .bind(body)
    .bind(if is_encrypted { 1 } else { 0 })
    .bind(file_name)
    .bind(file_data)
    .bind(content_type)
    .execute(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let id = result.last_insert_rowid();
    let created_at: (String,) = sqlx::query_as("SELECT created_at FROM chat_messages WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(AppError::Database)?;

    state.chat_service.invalidate_chat_for_room(room_id).await;
    state.chat_service.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "user".to_string(),
        file_name: Some(file_name.to_string()),
        file_content_type: Some(content_type.to_string()),
        is_encrypted,
    })
}

async fn persist_notification(
    state: &AppState,
    user: &User,
    room_id: i64,
    body: &str,
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, is_encrypted) VALUES (?, ?, ?, 'notification', 0)",
    )
    .bind(room_id)
    .bind(user.id)
    .bind(body)
    .execute(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let id = result.last_insert_rowid();
    let created_at: (String,) = sqlx::query_as("SELECT created_at FROM chat_messages WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(AppError::Database)?;

    state.chat_service.invalidate_chat_for_room(room_id).await;
    state.chat_service.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "notification".to_string(),
        file_name: None,
        file_content_type: None,
        is_encrypted: false,
    })
}

async fn update_read_position(
    state: &AppState,
    user_id: i64,
    room_id: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO chat_room_read_positions (room_id, user_id, last_read_message_id, updated_at)
        VALUES (?, ?, COALESCE((SELECT MAX(id) FROM chat_messages WHERE room_id = ?), 0), CURRENT_TIMESTAMP)
        ON CONFLICT(room_id, user_id) DO UPDATE SET
            last_read_message_id = COALESCE((SELECT MAX(id) FROM chat_messages WHERE room_id = excluded.room_id), 0),
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(room_id)
    .bind(user_id)
    .bind(room_id)
    .execute(&state.pool)
    .await
    .map_err(AppError::Database)?;

    state.chat_service.invalidate_chat_for_user(user_id).await;
    Ok(())
}

fn available_invitees(
    all_users: &[User],
    current_user_id: i64,
    participants: &[ChatParticipant],
) -> Vec<User> {
    let participant_ids: HashSet<i64> = participants
        .iter()
        .map(|participant| participant.id)
        .collect();
    all_users
        .iter()
        .filter(|user| user.id != current_user_id && !participant_ids.contains(&user.id))
        .cloned()
        .collect()
}

fn default_room_name(participants: &[User]) -> String {
    let joined_names = participants
        .iter()
        .map(|user| user.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("Chat with {}", joined_names)
}

fn room_path(room_id: i64, is_general: bool) -> String {
    if is_general || room_id == GENERAL_ROOM_ID {
        "/chat".to_string()
    } else {
        format!("/chat/rooms/{}", room_id)
    }
}

#[derive(Debug, Deserialize)]
struct ChatInput {
    body: Option<String>,
    typing: Option<bool>,
    file_data: Option<String>,
    file_name: Option<String>,
    file_content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StorePublicKeyInput {
    public_key: String,
}

#[derive(Debug, Serialize)]
pub struct PublicKeyResponse {
    public_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StoreRoomKeyInput {
    user_id: i64,
    encrypted_key: String,
}

#[derive(Debug, Serialize)]
pub struct RoomKeyResponse {
    encrypted_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RoomKeyMembersResponse {
    member_ids: Vec<i64>,
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
    Ok((StatusCode::OK, axum::http::header::HeaderMap::new(), body).into_response())
}

pub async fn store_public_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Json(input): axum::Json<StorePublicKeyInput>,
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
    Ok((StatusCode::OK, axum::http::header::HeaderMap::new(), body).into_response())
}

pub async fn store_room_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
    axum::Json(input): axum::Json<StoreRoomKeyInput>,
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
    Ok((StatusCode::OK, axum::http::header::HeaderMap::new(), body).into_response())
}

pub async fn serve_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    // Verify the requesting user is a member of the room this file belongs to.
    let row = sqlx::query_as::<_, (i64,)>("SELECT room_id FROM chat_messages WHERE id = ?")
        .bind(message_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(AppError::Database)?;

    let Some((room_id,)) = row else {
        return Err(AppError::NotFound(format!(
            "File for message {} not found",
            message_id
        )));
    };

    let Some(_room) = state
        .chat_service
        .get_room_for_user(ctx, user.id, room_id)
        .await?
    else {
        return Err(AppError::NotFound(format!(
            "File for message {} not found",
            message_id
        )));
    };

    if let Some((data, name, content_type)) =
        state.chat_service.get_chat_file(ctx, message_id).await?
    {
        let disposition = format!("inline; filename=\"{}\"", name.replace('"', "\\\""));
        return Ok((
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (axum::http::header::CONTENT_DISPOSITION, disposition),
                (
                    axum::http::header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".to_string(),
                ),
            ],
            data,
        )
            .into_response());
    }

    Err(AppError::NotFound(format!(
        "File for message {} not found",
        message_id
    )))
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
