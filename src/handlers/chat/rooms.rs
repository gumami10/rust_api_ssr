use super::{
    room_path, render_template, BroadcastEvent, ChatParticipant, ChatTemplate, CreateRoomForm,
    InviteForm, GENERAL_ROOM_ID,
};
use crate::error::AppError;
use crate::handlers::{auth, AppState};
use crate::models::user::User;
use axum::{
    extract::{Form, Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use std::collections::HashSet;

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

    if let Ok(event) = super::messages::persist_notification(
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

    let _ = state.chat_tx.send(BroadcastEvent::RoomChange {
        target_user_id: invited_user.id,
    });

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

    let invite = sqlx::query_as::<_, super::PendingInviteRow>(
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

    if let Ok(event) = super::messages::persist_notification(
        &state,
        &user,
        invite.room_id,
        &format!("{} joined the room", user.name),
    )
    .await
    {
        let _ = state.chat_tx.send(BroadcastEvent::Message(event));
    }

    let _ = state.chat_tx.send(BroadcastEvent::RoomChange {
        target_user_id: user.id,
    });

    Ok(Redirect::to(&room_path(invite.room_id, false)).into_response())
}

pub async fn render_chat_room_page(
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

    let _ = super::messages::update_read_position(state, viewer.id, room.id).await;

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
        .map(|user| user.display_name())
        .collect::<Vec<_>>()
        .join(", ");
    format!("Chat with {}", joined_names)
}
