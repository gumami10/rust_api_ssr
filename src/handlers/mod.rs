use crate::models::user::UserRepository;
use crate::services::users::UserService;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

pub mod api;
pub mod auth;
pub mod chat;
pub mod health;
pub mod views;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository + Send + Sync>,
    pub pool: SqlitePool,
    pub chat_tx: broadcast::Sender<crate::handlers::chat::ChatEvent>,
}

impl AppState {
    pub fn user_service(&self) -> UserService {
        UserService::new(Arc::clone(&self.user_repo))
    }
}
