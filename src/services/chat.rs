use crate::cache::AppCache;
use crate::context::QueryContext;
use crate::error::AppError;
use crate::handlers::chat::{
    ChatEvent, ChatParticipant, ChatRoomRow, ChatRoomView, PendingInviteRow, PendingInviteView,
};
use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Clone)]
pub struct ChatService {
    pool: SqlitePool,
    cache: AppCache,
}

impl ChatService {
    pub fn new(pool: SqlitePool, cache: AppCache) -> Self {
        Self { pool, cache }
    }

    pub async fn get_chat_messages(
        &self,
        ctx: QueryContext,
        room_id: i64,
    ) -> Result<Vec<ChatEvent>, AppError> {
        if !ctx.bypass_cache {
            if let Some(messages) = self.cache.chat_messages_by_room.get(&room_id).await {
                return Ok(messages);
            }
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
                chat_messages.file_content_type,
                chat_messages.is_encrypted
            FROM chat_messages
            INNER JOIN users ON users.id = chat_messages.user_id
            WHERE chat_messages.room_id = ?
            ORDER BY chat_messages.id DESC
            LIMIT 50
            "#,
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        let messages: Vec<ChatEvent> = messages.into_iter().rev().collect();
        self.cache
            .chat_messages_by_room
            .insert(room_id, messages.clone())
            .await;
        Ok(messages)
    }

    pub async fn get_room_participants(
        &self,
        ctx: QueryContext,
        room_id: i64,
    ) -> Result<Vec<ChatParticipant>, AppError> {
        if !ctx.bypass_cache {
            if let Some(participants) = self.cache.room_participants.get(&room_id).await {
                return Ok(participants);
            }
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
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        self.cache
            .room_participants
            .insert(room_id, participants.clone())
            .await;
        Ok(participants)
    }

    pub async fn get_accessible_rooms(
        &self,
        ctx: QueryContext,
        user_id: i64,
    ) -> Result<Vec<ChatRoomView>, AppError> {
        if !ctx.bypass_cache {
            if let Some(rooms) = self.cache.accessible_rooms.get(&user_id).await {
                return Ok(rooms);
            }
        }

        let rows = sqlx::query_as::<_, ChatRoomRow>(
            r#"
            SELECT
                rooms.id,
                rooms.name,
                rooms.kind = 'general' AS is_general,
                rooms.created_by_user_id,
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
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        let rooms: Vec<ChatRoomView> = rows
            .into_iter()
            .map(|row| ChatRoomView {
                path: if row.is_general || row.id == 1 {
                    "/chat".to_string()
                } else {
                    format!("/chat/rooms/{}", row.id)
                },
                is_active: false,
                id: row.id,
                name: row.name,
                is_general: row.is_general,
                created_by_user_id: row.created_by_user_id,
                participant_count: row.participant_count,
                unread_count: 0,
                is_encrypted: !row.is_general && row.participant_count == 2,
            })
            .collect();

        self.cache
            .accessible_rooms
            .insert(user_id, rooms.clone())
            .await;
        Ok(rooms)
    }

    pub async fn get_room_for_user(
        &self,
        ctx: QueryContext,
        user_id: i64,
        room_id: i64,
    ) -> Result<Option<ChatRoomView>, AppError> {
        if !ctx.bypass_cache {
            if let Some(room) = self.cache.accessible_room.get(&(user_id, room_id)).await {
                return Ok(room);
            }
        }

        let base_rooms = self.get_accessible_rooms(ctx, user_id).await?;
        let room = base_rooms
            .into_iter()
            .find(|r| r.id == room_id)
            .map(|mut room| {
                room.is_active = true;
                room
            });

        self.cache
            .accessible_room
            .insert((user_id, room_id), room.clone())
            .await;
        Ok(room)
    }

    pub async fn get_unread_counts(
        &self,
        ctx: QueryContext,
        user_id: i64,
    ) -> Result<HashMap<i64, i64>, AppError> {
        if !ctx.bypass_cache {
            if let Some(counts) = self.cache.unread_counts.get(&user_id).await {
                return Ok(counts);
            }
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
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)?;

        let counts: HashMap<i64, i64> = rows.into_iter().collect();
        self.cache
            .unread_counts
            .insert(user_id, counts.clone())
            .await;
        Ok(counts)
    }

    pub async fn get_pending_invites(
        &self,
        ctx: QueryContext,
        user_id: i64,
    ) -> Result<Vec<PendingInviteView>, AppError> {
        if !ctx.bypass_cache {
            if let Some(invites) = self.cache.pending_invites.get(&user_id).await {
                return Ok(invites);
            }
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
        .fetch_all(&self.pool)
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

        self.cache
            .pending_invites
            .insert(user_id, views.clone())
            .await;
        Ok(views)
    }

    pub async fn get_chat_file(
        &self,
        ctx: QueryContext,
        message_id: i64,
    ) -> Result<Option<(Vec<u8>, String, String)>, AppError> {
        if !ctx.bypass_cache {
            if let Some(file) = self.cache.chat_file.get(&message_id).await {
                return Ok(Some(file));
            }
        }

        let row = sqlx::query_as::<_, (Vec<u8>, String, String)>(
            "SELECT file_data, file_name, file_content_type FROM chat_messages WHERE id = ? AND file_data IS NOT NULL",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;

        if let Some(ref file) = row {
            self.cache.chat_file.insert(message_id, file.clone()).await;
        }

        Ok(row)
    }

    // Invalidation helpers
    pub async fn invalidate_chat_for_room(&self, room_id: i64) {
        self.cache.invalidate_chat_for_room(room_id).await;
    }

    pub async fn invalidate_chat_for_user(&self, user_id: i64) {
        self.cache.invalidate_chat_for_user(user_id).await;
    }

    pub async fn invalidate_accessible_room(&self, user_id: i64, room_id: i64) {
        self.cache
            .invalidate_accessible_room(user_id, room_id)
            .await;
    }

    pub async fn invalidate_all_unread_counts(&self) {
        self.cache.invalidate_all_unread_counts().await;
    }

    pub async fn invalidate_pending_invites(&self, user_id: i64) {
        self.cache.pending_invites.invalidate(&user_id).await;
    }

    pub async fn invalidate_user(&self, email: &str, id: i64) {
        self.cache.invalidate_user(email, id).await;
    }

    pub async fn invalidate_all_users(&self) {
        self.cache.invalidate_all_users().await;
    }

    // ---- E2E encryption helpers ----

    pub async fn get_public_key(
        &self,
        _ctx: QueryContext,
        user_id: i64,
    ) -> Result<Option<String>, AppError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT public_key FROM user_public_keys WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;
        Ok(row.map(|r| r.0))
    }

    pub async fn store_public_key(&self, user_id: i64, public_key: &str) -> Result<(), AppError> {
        sqlx::query(
            r#"
            INSERT INTO user_public_keys (user_id, public_key)
            VALUES (?, ?)
            ON CONFLICT(user_id) DO UPDATE SET public_key = excluded.public_key
            "#,
        )
        .bind(user_id)
        .bind(public_key)
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn get_encrypted_room_key(
        &self,
        _ctx: QueryContext,
        room_id: i64,
        user_id: i64,
    ) -> Result<Option<String>, AppError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT encrypted_key FROM chat_room_keys WHERE room_id = ? AND user_id = ?",
        )
        .bind(room_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;
        Ok(row.map(|r| r.0))
    }

    pub async fn store_encrypted_room_key(
        &self,
        room_id: i64,
        user_id: i64,
        encrypted_key: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            INSERT INTO chat_room_keys (room_id, user_id, encrypted_key)
            VALUES (?, ?, ?)
            ON CONFLICT(room_id, user_id) DO UPDATE SET encrypted_key = excluded.encrypted_key
            "#,
        )
        .bind(room_id)
        .bind(user_id)
        .bind(encrypted_key)
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)?;
        Ok(())
    }

    pub async fn is_room_member(
        &self,
        _ctx: QueryContext,
        room_id: i64,
        user_id: i64,
    ) -> Result<bool, AppError> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT 1 FROM chat_room_members WHERE room_id = ? AND user_id = ?",
        )
        .bind(room_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)?;
        Ok(row.is_some())
    }

    pub async fn get_room_key_member_ids(
        &self,
        _ctx: QueryContext,
        room_id: i64,
    ) -> Result<Vec<i64>, AppError> {
        let rows =
            sqlx::query_as::<_, (i64,)>("SELECT user_id FROM chat_room_keys WHERE room_id = ?")
                .bind(room_id)
                .fetch_all(&self.pool)
                .await
                .map_err(AppError::Database)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
