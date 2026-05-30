use super::{BroadcastEvent, RoomQuery, GENERAL_ROOM_ID};
use crate::error::AppError;
use crate::handlers::{auth, AppState};
use crate::models::user::User;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

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
                            if let Some(is_typing) = input.typing {
                                let _ = state.chat_tx.send(BroadcastEvent::Typing {
                                    room_id,
                                    user_name: user.name.clone(),
                                    is_typing,
                                });
                                continue;
                            }

                            let body = input.body.as_deref().unwrap_or("").trim().to_string();

                            if let (Some(ref file_b64), Some(ref file_name)) = (&input.file_data, &input.file_name) {
                                let content_type = input.file_content_type.clone().unwrap_or_else(|| "application/octet-stream".to_string());
                                if let Ok(file_bytes) = base64::engine::general_purpose::STANDARD.decode(file_b64) {
                                    let display_body = if body.is_empty() {
                                        format!("shared a file: {}", file_name)
                                    } else {
                                        body.clone()
                                    };
                                    if let Ok(event) = super::messages::persist_message_with_file(
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

                            if let Ok(event) = super::messages::persist_message(&state, &user, room_id, &body, is_encrypted).await {
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
                    Ok(BroadcastEvent::RoomChange { target_user_id }) => *target_user_id == user.id,
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

#[derive(Debug, Deserialize)]
struct ChatInput {
    body: Option<String>,
    typing: Option<bool>,
    file_data: Option<String>,
    file_name: Option<String>,
    file_content_type: Option<String>,
}
