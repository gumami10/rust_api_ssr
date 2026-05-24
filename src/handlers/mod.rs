use crate::models::user::UserRepository;
use crate::services::chat::ChatService;
use crate::services::users::UserService;
use axum::http::HeaderMap;
use sqlx::SqlitePool;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

pub fn query_context(headers: &HeaderMap) -> crate::context::QueryContext {
    let bypass = headers
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("no-cache"))
        .unwrap_or(false);
    crate::context::QueryContext {
        bypass_cache: bypass,
    }
}

pub mod api;
pub mod auth;
pub mod chat;
pub mod health;
pub mod views;

#[derive(Clone, Debug)]
pub struct RequestMetric {
    pub path: String,
    pub elapsed_ms: f64,
}

#[derive(Clone, Default)]
pub struct RequestMetrics {
    inner: Arc<Mutex<VecDeque<RequestMetric>>>,
}

impl RequestMetrics {
    const MAX_ENTRIES: usize = 200;

    pub fn record(&self, path: impl Into<String>, elapsed_ms: f64) {
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
            .take(8)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<RequestMetric> {
        self.inner
            .lock()
            .expect("request metrics lock")
            .iter()
            .cloned()
            .collect()
    }
}

#[derive(Clone)]
pub struct LoginRateLimiter {
    inner: Arc<Mutex<HashMap<String, (usize, Instant)>>>,
    max_attempts: usize,
    window: Duration,
}

impl LoginRateLimiter {
    pub fn new(max_attempts: usize, window_secs: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_attempts,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn is_allowed(&self, key: &str) -> bool {
        let map = self.inner.lock().expect("rate limiter lock");
        let now = Instant::now();
        if let Some((count, start)) = map.get(key) {
            if now.duration_since(*start) < self.window {
                return *count < self.max_attempts;
            }
        }
        true
    }

    pub fn record_attempt(&self, key: &str) {
        let mut map = self.inner.lock().expect("rate limiter lock");
        let now = Instant::now();
        if let Some((count, start)) = map.get_mut(key) {
            if now.duration_since(*start) >= self.window {
                *count = 1;
                *start = now;
            } else {
                *count += 1;
            }
        } else {
            map.insert(key.to_string(), (1, now));
        }
    }

    pub fn clear(&self, key: &str) {
        let mut map = self.inner.lock().expect("rate limiter lock");
        map.remove(key);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub user_repo: Arc<dyn UserRepository + Send + Sync>,
    pub user_service: UserService,
    pub chat_service: ChatService,
    pub pool: SqlitePool,
    pub chat_tx: broadcast::Sender<crate::handlers::chat::BroadcastEvent>,
    pub request_metrics: RequestMetrics,
    pub cookie_secure: bool,
    pub login_rate_limiter: LoginRateLimiter,
}
