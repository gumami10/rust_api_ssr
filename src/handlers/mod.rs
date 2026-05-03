use crate::models::user::UserRepository;
use crate::services::users::UserService;
use sqlx::SqlitePool;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::Arc;
use tokio::sync::broadcast;

pub mod api;
pub mod auth;
pub mod chat;
pub mod health;
pub mod views;

#[derive(Clone, Debug)]
pub struct RequestMetric {
    pub path: String,
    pub elapsed_ms: u64,
}

#[derive(Clone, Default)]
pub struct RequestMetrics {
    inner: Arc<Mutex<VecDeque<RequestMetric>>>,
}

impl RequestMetrics {
    const MAX_ENTRIES: usize = 8;

    pub fn record(&self, path: impl Into<String>, elapsed_ms: u64) {
        let mut entries = self.inner.lock().expect("request metrics lock");
        entries.push_front(RequestMetric {
            path: path.into(),
            elapsed_ms,
        });

        while entries.len() > Self::MAX_ENTRIES {
            entries.pop_back();
        }
    }

    pub fn recent(&self) -> Vec<RequestMetric> {
        self.inner
            .lock()
            .expect("request metrics lock")
            .iter()
            .cloned()
            .collect()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository + Send + Sync>,
    pub pool: SqlitePool,
    pub chat_tx: broadcast::Sender<crate::handlers::chat::BroadcastEvent>,
    pub request_metrics: RequestMetrics,
}

impl AppState {
    pub fn user_service(&self) -> UserService {
        UserService::new(Arc::clone(&self.user_repo))
    }
}
