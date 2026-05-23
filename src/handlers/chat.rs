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
use std::collections::{HashMap, HashSet};
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
    pub participant_count: i64,
}

#[derive(Debug, Clone)]
pub struct ChatRoomView {
    pub id: i64,
    pub name: String,
    pub is_general: bool,
    pub participant_count: i64,
    pub path: String,
    pub is_active: bool,
    pub unread_count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct ChatParticipant {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, FromRow)]
struct PendingInviteRow {
    id: i64,
    room_id: i64,
    room_name: String,
    invited_by_name: String,
    created_at: String,
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
    all_users: Vec<User>,
    available_invitees: Vec<User>,
    pending_invites: Vec<PendingInviteView>,
    error: Option<String>,
}

pub async fn render_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    render_chat_room_page(&state, user, GENERAL_ROOM_ID, None).await
}

pub async fn render_chat_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    render_chat_room_page(&state, user, room_id, None).await
}

pub async fn create_chat_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CreateRoomForm>,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
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
        )
        .await;
    }

    let selected_users = users_by_ids(&state, &participant_ids).await?;
    if selected_users.len() != participant_ids.len() {
        return render_chat_room_page(
            &state,
            user,
            GENERAL_ROOM_ID,
            Some("One or more selected users could not be found.".to_string()),
        )
        .await;
    }

    let room_name = form
        .name
        .unwrap_or_default()
        .trim()
        .to_string();
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
        state.cache.invalidate_chat_for_user(*pid).await;
        state
            .cache
            .invalidate_accessible_room(*pid, room_id)
            .await;
    }
    state.cache.invalidate_chat_for_user(user.id).await;
    state
        .cache
        .invalidate_accessible_room(user.id, room_id)
        .await;
    state.cache.invalidate_chat_for_room(room_id).await;

    // Emit notification
    if let Ok(event) = persist_notification(
        &state, &user, room_id,
        &format!("{} created this room", user.name),
    ).await {
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
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let Some(room) = accessible_room(&state, user.id, room_id).await? else {
        return Ok(Redirect::to("/chat").into_response());
    };

    if room.is_general {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("The general chat cannot be restricted by invitation.".to_string()),
        )
        .await;
    }

    let invited_user = state
        .user_service()
        .get_user(form.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User with id {} not found", form.user_id)))?;

    if invited_user.id == user.id {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("You cannot invite yourself.".to_string()),
        )
        .await;
    }

    let participants = room_participants(&state, room.id).await?;
    if participants.iter().any(|participant| participant.id == invited_user.id) {
        return render_chat_room_page(
            &state,
            user,
            room_id,
            Some("That user is already part of this chat.".to_string()),
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
        )
        .await;
    }

    let member_ids: HashSet<i64> = participants.into_iter().map(|participant| participant.id).collect();
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

    state.cache.pending_invites.invalidate(&invited_user.id).await;

    Ok(Redirect::to(&room.path).into_response())
}

pub async fn accept_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
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
        return Err(AppError::NotFound(format!("Invite with id {} not found", invite_id)));
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

    state.cache.invalidate_chat_for_user(user.id).await;
    state
        .cache
        .invalidate_accessible_room(user.id, invite.room_id)
        .await;
    state.cache.invalidate_chat_for_room(invite.room_id).await;
    state.cache.invalidate_all_unread_counts().await;

    // Emit notification
    if let Ok(event) = persist_notification(
        &state, &user, invite.room_id,
        &format!("{} joined the room", user.name),
    ).await {
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
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let room_id = query.room_id.unwrap_or(GENERAL_ROOM_ID);
    let Some(room) = accessible_room(&state, user.id, room_id).await? else {
        return Ok(Redirect::to("/chat").into_response());
    };

    Ok(ws
        .on_upgrade(move |socket| handle_socket(socket, state, user, room.id))
        .into_response())
}

async fn handle_socket(socket: WebSocket, state: AppState, user: User, room_id: i64) {
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
                                        file_name, &file_bytes, &content_type,
                                    ).await {
                                        let _ = state.chat_tx.send(BroadcastEvent::Message(event));
                                    }
                                }
                                continue;
                            }

                            if body.is_empty() {
                                continue;
                            }

                            if let Ok(event) = persist_message(&state, &user, room_id, &body).await {
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
) -> Result<Response, AppError> {
    let Some(room) = accessible_room(state, viewer.id, room_id).await? else {
        return Ok(Redirect::to("/chat").into_response());
    };

    // Mark current room as read
    let _ = update_read_position(state, viewer.id, room.id).await;

    let request_metrics = state.request_metrics.recent();
    let messages = recent_messages(state, room.id).await?;
    let participants = room_participants(state, room.id).await?;
    let all_users = state
        .user_service()
        .list_users()
        .await?
        .into_iter()
        .filter(|candidate| candidate.id != viewer.id)
        .collect::<Vec<_>>();
    let pending_invites = pending_invites_for_user(state, viewer.id).await?;
    let unread_counts = get_unread_counts(state, viewer.id).await?;
    let rooms = accessible_rooms(state, viewer.id, room.id, &unread_counts).await?;
    let available_invitees = available_invitees(&all_users, viewer.id, &participants);

    render_template(
        ChatTemplate {
            viewer: Some(viewer.clone()),
            request_metrics,
            user: viewer,
            room,
            rooms,
            messages,
            participants,
            all_users,
            available_invitees,
            pending_invites,
            error,
        },
        StatusCode::OK,
    )
}

async fn accessible_room(
    state: &AppState,
    user_id: i64,
    room_id: i64,
) -> Result<Option<ChatRoomView>, AppError> {
    if let Some(room) = state.cache.accessible_room.get(&(user_id, room_id)).await {
        return Ok(room);
    }

    let base_rooms = accessible_rooms_base(state, user_id).await?;
    let room = base_rooms.into_iter().find(|r| r.id == room_id).map(|mut room| {
        room.is_active = true;
        room
    });

    state
        .cache
        .accessible_room
        .insert((user_id, room_id), room.clone())
        .await;
    Ok(room)
}

async fn accessible_rooms_base(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<ChatRoomView>, AppError> {
    if let Some(rooms) = state.cache.accessible_rooms.get(&user_id).await {
        return Ok(rooms);
    }

    let rows = sqlx::query_as::<_, ChatRoomRow>(
        r#"
        SELECT
            rooms.id,
            rooms.name,
            rooms.kind = 'general' AS is_general,
            (SELECT COUNT(*) FROM chat_room_members members WHERE members.room_id = rooms.id) AS participant_count
        FROM chat_rooms rooms
        WHERE rooms.kind = 'general'
           OR EXISTS (
               SELECT 1
               FROM chat_room_members members
               WHERE members.room_id = rooms.id
                 AND members.user_id = ?
           )
        ORDER BY rooms.kind = 'general' DESC, rooms.created_at DESC, rooms.id DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let rooms: Vec<ChatRoomView> = rows
        .into_iter()
        .map(|row| ChatRoomView {
            path: room_path(row.id, row.is_general),
            is_active: false,
            id: row.id,
            name: row.name,
            is_general: row.is_general,
            participant_count: row.participant_count,
            unread_count: 0,
        })
        .collect();

    state.cache.accessible_rooms.insert(user_id, rooms.clone()).await;
    Ok(rooms)
}

async fn accessible_rooms(
    state: &AppState,
    user_id: i64,
    active_room_id: i64,
    unread_counts: &HashMap<i64, i64>,
) -> Result<Vec<ChatRoomView>, AppError> {
    let mut rooms = accessible_rooms_base(state, user_id).await?;
    for room in &mut rooms {
        room.is_active = room.id == active_room_id;
        room.unread_count = if room.id == active_room_id {
            0
        } else {
            unread_counts.get(&room.id).copied().unwrap_or(0)
        };
    }
    Ok(rooms)
}

async fn room_participants(
    state: &AppState,
    room_id: i64,
) -> Result<Vec<ChatParticipant>, AppError> {
    if let Some(participants) = state.cache.room_participants.get(&room_id).await {
        return Ok(participants);
    }

    let participants = sqlx::query_as::<_, ChatParticipant>(
        r#"
        SELECT users.id, users.name, users.email
        FROM chat_room_members
        INNER JOIN users ON users.id = chat_room_members.user_id
        WHERE chat_room_members.room_id = ?
        ORDER BY users.name
        "#,
    )
    .bind(room_id)
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    state
        .cache
        .room_participants
        .insert(room_id, participants.clone())
        .await;
    Ok(participants)
}

async fn pending_invites_for_user(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<PendingInviteView>, AppError> {
    if let Some(invites) = state.cache.pending_invites.get(&user_id).await {
        return Ok(invites);
    }

    let invites = sqlx::query_as::<_, PendingInviteRow>(
        r#"
        SELECT
            invites.id,
            invites.room_id,
            rooms.name AS room_name,
            inviter.name AS invited_by_name,
            invites.created_at
        FROM chat_room_invites invites
        INNER JOIN chat_rooms rooms ON rooms.id = invites.room_id
        INNER JOIN users inviter ON inviter.id = invites.invited_by_user_id
        WHERE invites.invited_user_id = ? AND invites.status = 'pending'
        ORDER BY invites.created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let views: Vec<PendingInviteView> = invites
        .into_iter()
        .map(|invite| PendingInviteView {
            accept_path: format!("/chat/invites/{}/accept", invite.id),
            room_name: invite.room_name,
            invited_by_name: invite.invited_by_name,
            created_at: invite.created_at,
        })
        .collect();

    state.cache.pending_invites.insert(user_id, views.clone()).await;
    Ok(views)
}

async fn recent_messages(
    state: &AppState,
    room_id: i64,
) -> Result<Vec<ChatEvent>, AppError> {
    if let Some(messages) = state.cache.chat_messages_by_room.get(&room_id).await {
        return Ok(messages);
    }

    let messages = sqlx::query_as::<_, ChatEvent>(
        r#"
        SELECT
            chat_messages.id,
            chat_messages.room_id,
            users.name AS user_name,
            chat_messages.body,
            chat_messages.created_at,
            chat_messages.kind,
            chat_messages.file_name,
            chat_messages.file_content_type
        FROM chat_messages
        INNER JOIN users ON users.id = chat_messages.user_id
        WHERE chat_messages.room_id = ?
        ORDER BY chat_messages.id DESC
        LIMIT 50
        "#,
    )
    .bind(room_id)
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let messages: Vec<ChatEvent> = messages.into_iter().rev().collect();
    state
        .cache
        .chat_messages_by_room
        .insert(room_id, messages.clone())
        .await;
    Ok(messages)
}

async fn persist_message(
    state: &AppState,
    user: &User,
    room_id: i64,
    body: &str,
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query("INSERT INTO chat_messages (room_id, user_id, body, kind) VALUES (?, ?, ?, 'user')")
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

    state.cache.invalidate_chat_for_room(room_id).await;
    state.cache.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "user".to_string(),
        file_name: None,
        file_content_type: None,
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
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind, file_name, file_data, file_content_type) VALUES (?, ?, ?, 'user', ?, ?, ?)",
    )
    .bind(room_id)
    .bind(user.id)
    .bind(body)
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

    state.cache.invalidate_chat_for_room(room_id).await;
    state.cache.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "user".to_string(),
        file_name: Some(file_name.to_string()),
        file_content_type: Some(content_type.to_string()),
    })
}

async fn persist_notification(
    state: &AppState,
    user: &User,
    room_id: i64,
    body: &str,
) -> Result<ChatEvent, AppError> {
    let result = sqlx::query(
        "INSERT INTO chat_messages (room_id, user_id, body, kind) VALUES (?, ?, ?, 'notification')",
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

    state.cache.invalidate_chat_for_room(room_id).await;
    state.cache.invalidate_all_unread_counts().await;

    Ok(ChatEvent {
        id,
        room_id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
        kind: "notification".to_string(),
        file_name: None,
        file_content_type: None,
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

    state.cache.unread_counts.invalidate(&user_id).await;
    Ok(())
}

async fn get_unread_counts(
    state: &AppState,
    user_id: i64,
) -> Result<HashMap<i64, i64>, AppError> {
    if let Some(counts) = state.cache.unread_counts.get(&user_id).await {
        return Ok(counts);
    }

    let rows = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT rooms.id,
               (SELECT COUNT(*) FROM chat_messages
                WHERE chat_messages.room_id = rooms.id
                  AND chat_messages.id > COALESCE(
                      (SELECT last_read_message_id FROM chat_room_read_positions
                       WHERE room_id = rooms.id AND user_id = ?), 0)
               ) AS unread
        FROM chat_rooms rooms
        WHERE rooms.kind = 'general'
           OR EXISTS (SELECT 1 FROM chat_room_members WHERE room_id = rooms.id AND user_id = ?)
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let counts: HashMap<i64, i64> = rows.into_iter().collect();
    state.cache.unread_counts.insert(user_id, counts.clone()).await;
    Ok(counts)
}

async fn users_by_ids(state: &AppState, ids: &[i64]) -> Result<Vec<User>, AppError> {
    let mut users = Vec::with_capacity(ids.len());
    let mut missing = Vec::new();

    for id in ids {
        if let Some(user) = state.cache.user_by_id.get(id).await {
            users.push(user);
        } else {
            missing.push(*id);
        }
    }

    if !missing.is_empty() {
        let placeholders = missing.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let query = format!("SELECT id, name, email FROM users WHERE id IN ({})", placeholders);
        let mut request = sqlx::query_as::<_, User>(&query);

        for id in &missing {
            request = request.bind(id);
        }

        let fetched = request.fetch_all(&state.pool).await.map_err(AppError::Database)?;
        for user in &fetched {
            state.cache.user_by_id.insert(user.id, user.clone()).await;
        }
        users.extend(fetched);
    }

    Ok(users)
}

fn available_invitees(all_users: &[User], current_user_id: i64, participants: &[ChatParticipant]) -> Vec<User> {
    let participant_ids: HashSet<i64> = participants.iter().map(|participant| participant.id).collect();
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

pub async fn serve_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(_user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    if let Some((data, name, content_type)) = state.cache.chat_file.get(&message_id).await {
        let disposition = format!("inline; filename=\"{}\"", name.replace('"', "\\\""));
        return Ok((
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (axum::http::header::CONTENT_DISPOSITION, disposition),
                (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
            ],
            data,
        ).into_response());
    }

    let row = sqlx::query_as::<_, (Vec<u8>, String, String)>(
        "SELECT file_data, file_name, file_content_type FROM chat_messages WHERE id = ? AND file_data IS NOT NULL",
    )
    .bind(message_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(AppError::Database)?;

    let Some((data, name, content_type)) = row else {
        return Err(AppError::NotFound(format!("File for message {} not found", message_id)));
    };

    state
        .cache
        .chat_file
        .insert(message_id, (data.clone(), name.clone(), content_type.clone()))
        .await;

    let disposition = format!("inline; filename=\"{}\"", name.replace('"', "\\\""));
    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (axum::http::header::CONTENT_DISPOSITION, disposition),
            (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
        ],
        data,
    ).into_response())
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
