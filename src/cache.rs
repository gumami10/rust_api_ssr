use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;

use crate::handlers::chat::{ChatEvent, ChatParticipant, ChatRoomView, PendingInviteView};
use crate::models::user::{NewUser, UpdateUser, User, UserRepository, UserWithPassword};

#[derive(Clone)]
pub struct AppCache {
    pub user_by_id: Cache<i64, User>,
    pub user_by_email: Cache<String, Option<User>>,
    pub user_with_password_by_email: Cache<String, Option<UserWithPassword>>,
    pub all_users: Cache<String, Vec<User>>,
    pub session_by_token: Cache<String, Option<User>>,
    pub chat_messages_by_room: Cache<i64, Vec<ChatEvent>>,
    pub room_participants: Cache<i64, Vec<ChatParticipant>>,
    pub accessible_rooms: Cache<i64, Vec<ChatRoomView>>,
    pub accessible_room: Cache<(i64, i64), Option<ChatRoomView>>,
    pub unread_counts: Cache<i64, HashMap<i64, i64>>,
    pub pending_invites: Cache<i64, Vec<PendingInviteView>>,
    pub chat_file: Cache<i64, (Vec<u8>, String, String)>,
}

impl Default for AppCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCache {
    pub fn new() -> Self {
        Self {
            user_by_id: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
            user_by_email: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
            user_with_password_by_email: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
            all_users: Cache::builder()
                .max_capacity(1)
                .time_to_live(Duration::from_secs(60))
                .build(),
            session_by_token: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
            chat_messages_by_room: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            room_participants: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(120))
                .build(),
            accessible_rooms: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
            accessible_room: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
            unread_counts: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(15))
                .build(),
            pending_invites: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
            chat_file: Cache::builder()
                .max_capacity(5_000)
                .time_to_live(Duration::from_secs(86400))
                .build(),
        }
    }

    pub async fn invalidate_user(&self, email: &str, id: i64) {
        self.user_by_id.invalidate(&id).await;
        self.user_by_email.invalidate(email).await;
        self.user_with_password_by_email.invalidate(email).await;
    }

    pub async fn invalidate_all_users(&self) {
        self.all_users.invalidate_all();
    }

    pub async fn invalidate_chat_for_room(&self, room_id: i64) {
        self.chat_messages_by_room.invalidate(&room_id).await;
        self.room_participants.invalidate(&room_id).await;
    }

    pub async fn invalidate_chat_for_user(&self, user_id: i64) {
        self.accessible_rooms.invalidate(&user_id).await;
        self.unread_counts.invalidate(&user_id).await;
        self.pending_invites.invalidate(&user_id).await;
    }

    pub async fn invalidate_accessible_room(&self, user_id: i64, room_id: i64) {
        self.accessible_room.invalidate(&(user_id, room_id)).await;
    }

    pub async fn invalidate_all_accessible_rooms(&self) {
        self.accessible_rooms.invalidate_all();
        self.accessible_room.invalidate_all();
    }

    pub async fn invalidate_all_unread_counts(&self) {
        self.unread_counts.invalidate_all();
    }
}

pub struct CachedUserRepository {
    inner: Arc<dyn UserRepository + Send + Sync>,
    cache: AppCache,
}

impl CachedUserRepository {
    pub fn new(inner: Arc<dyn UserRepository + Send + Sync>, cache: AppCache) -> Self {
        Self { inner, cache }
    }
}

#[async_trait::async_trait]
impl UserRepository for CachedUserRepository {
    async fn get_user_by_id(
        &self,
        ctx: crate::context::QueryContext,
        id: i64,
    ) -> Result<Option<User>, sqlx::Error> {
        if !ctx.bypass_cache {
            if let Some(user) = self.cache.user_by_id.get(&id).await {
                return Ok(Some(user));
            }
        }
        let user = self.inner.get_user_by_id(ctx, id).await?;
        if let Some(ref user) = user {
            self.cache.user_by_id.insert(id, user.clone()).await;
        }
        Ok(user)
    }

    async fn get_user_by_email(
        &self,
        ctx: crate::context::QueryContext,
        email: &str,
    ) -> Result<Option<User>, sqlx::Error> {
        if !ctx.bypass_cache {
            if let Some(user) = self.cache.user_by_email.get(email).await {
                return Ok(user);
            }
        }
        let user = self.inner.get_user_by_email(ctx, email).await?;
        self.cache
            .user_by_email
            .insert(email.to_string(), user.clone())
            .await;
        Ok(user)
    }

    async fn get_user_with_password_by_email(
        &self,
        ctx: crate::context::QueryContext,
        email: &str,
    ) -> Result<Option<UserWithPassword>, sqlx::Error> {
        if !ctx.bypass_cache {
            if let Some(user) = self.cache.user_with_password_by_email.get(email).await {
                return Ok(user);
            }
        }
        let user = self.inner.get_user_with_password_by_email(ctx, email).await?;
        self.cache
            .user_with_password_by_email
            .insert(email.to_string(), user.clone())
            .await;
        Ok(user)
    }

    async fn list_users(
        &self,
        ctx: crate::context::QueryContext,
    ) -> Result<Vec<User>, sqlx::Error> {
        if !ctx.bypass_cache {
            if let Some(users) = self.cache.all_users.get("all").await {
                return Ok(users);
            }
        }
        let users = self.inner.list_users(ctx).await?;
        self.cache
            .all_users
            .insert("all".to_string(), users.clone())
            .await;
        Ok(users)
    }

    async fn create_user(&self, user: NewUser) -> Result<User, sqlx::Error> {
        let result = self.inner.create_user(user).await?;
        self.cache.invalidate_user(&result.email, result.id).await;
        self.cache.invalidate_all_users().await;
        Ok(result)
    }

    async fn update_user(
        &self,
        id: i64,
        user: UpdateUser,
    ) -> Result<Option<User>, sqlx::Error> {
        let old_email = match self.cache.user_by_id.get(&id).await {
            Some(u) => Some(u.email),
            None => self.inner.get_user_by_id(crate::context::QueryContext::default(), id).await?.map(|u| u.email),
        };

        if let Some(email) = old_email {
            self.cache.invalidate_user(&email, id).await;
        }

        let result = self.inner.update_user(id, user).await?;

        if let Some(ref user) = result {
            self.cache.invalidate_user(&user.email, user.id).await;
        }
        self.cache.invalidate_all_users().await;
        Ok(result)
    }

    async fn delete_user(&self, id: i64) -> Result<bool, sqlx::Error> {
        let old_email = match self.cache.user_by_id.get(&id).await {
            Some(u) => Some(u.email),
            None => self.inner.get_user_by_id(crate::context::QueryContext::default(), id).await?.map(|u| u.email),
        };

        if let Some(email) = old_email {
            self.cache.invalidate_user(&email, id).await;
        }

        let result = self.inner.delete_user(id).await?;
        self.cache.invalidate_all_users().await;
        Ok(result)
    }
}
