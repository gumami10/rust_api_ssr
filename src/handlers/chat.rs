use crate::error::AppError;
use crate::handlers::{auth, AppState};
use crate::models::user::User;
use askama::Template;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::HeaderMap,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatEvent {
    pub id: i64,
    pub user_name: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
struct ChatInput {
    body: String,
}

#[derive(Template)]
#[template(path = "chat/index.html")]
struct ChatTemplate {
    user: User,
    messages: Vec<ChatEvent>,
}

pub async fn render_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    let messages = recent_messages(&state).await?;
    render_template(
        ChatTemplate { user, messages },
        StatusCode::OK,
    )
}

pub async fn chat_ws(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let Some(user) = auth::current_user(&state, &headers).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, user)).into_response())
}

async fn handle_socket(socket: WebSocket, state: AppState, user: User) {
    let (mut sender, mut receiver) = socket.split();
    let mut broadcast_rx = state.chat_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(payload))) => {
                        if let Ok(input) = serde_json::from_str::<ChatInput>(payload.as_str()) {
                            let body = input.body.trim().to_string();
                            if body.is_empty() {
                                continue;
                            }

                            if let Ok(event) = persist_message(&state, &user, &body).await {
                                let _ = state.chat_tx.send(event);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            broadcasted = broadcast_rx.recv() => {
                match broadcasted {
                    Ok(event) => {
                        let payload = serde_json::to_string(&event).unwrap_or_default();
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn recent_messages(state: &AppState) -> Result<Vec<ChatEvent>, AppError> {
    let messages = sqlx::query_as::<_, ChatEvent>(
        r#"
        SELECT chat_messages.id, users.name AS user_name, chat_messages.body, chat_messages.created_at
        FROM chat_messages
        INNER JOIN users ON users.id = chat_messages.user_id
        ORDER BY chat_messages.id DESC
        LIMIT 50
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::Database)?;

    Ok(messages.into_iter().rev().collect())
}

async fn persist_message(state: &AppState, user: &User, body: &str) -> Result<ChatEvent, AppError> {
    let result = sqlx::query("INSERT INTO chat_messages (user_id, body) VALUES (?, ?)")
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

    Ok(ChatEvent {
        id,
        user_name: user.name.clone(),
        body: body.to_string(),
        created_at: created_at.0,
    })
}

fn render_template<T: Template>(template: T, status: StatusCode) -> Result<Response, AppError> {
    let html = template.render().map_err(|_| AppError::Internal)?;
    Ok((status, Html(html)).into_response())
}
