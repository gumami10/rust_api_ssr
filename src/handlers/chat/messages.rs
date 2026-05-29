use super::ChatEvent;
use crate::error::AppError;
use crate::handlers::{auth, AppState};
use crate::models::user::User;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};

pub async fn persist_message(
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

pub async fn persist_message_with_file(
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

pub async fn persist_notification(
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

pub async fn update_read_position(
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

pub async fn serve_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(message_id): Path<i64>,
) -> Result<Response, AppError> {
    let ctx = crate::handlers::query_context(&headers);
    let Some(user) = auth::current_user(&state, &headers, ctx).await? else {
        return Ok(Redirect::to("/login").into_response());
    };

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
