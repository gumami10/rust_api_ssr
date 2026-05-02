use crate::models::user::UserRepository;
use std::sync::Arc;

pub mod api;
pub mod views;

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository + Send + Sync>,
}
