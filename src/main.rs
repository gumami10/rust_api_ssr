use rust_api_ssr::app::create_router;
use rust_api_ssr::config::Config;
use rust_api_ssr::handlers::{AppState, RequestMetrics};
use rust_api_ssr::models::user::SqliteUserRepository;
use rust_api_ssr::services::chat::ChatService;
use rust_api_ssr::services::users::UserService;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "rust_api_ssr=info,axum=warn".into()),
        ))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = Config::from_env();

    info!("Connecting to database...");
    let connect_options =
        SqliteConnectOptions::from_str(&config.database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options)
        .await?;

    ensure_schema(&pool).await?;

    let cache = rust_api_ssr::cache::AppCache::new();
    let user_repo: Arc<dyn rust_api_ssr::models::user::UserRepository + Send + Sync> = Arc::new(
        rust_api_ssr::cache::CachedUserRepository::new(
            Arc::new(SqliteUserRepository::new(pool.clone())),
            cache.clone(),
        ),
    );
    let user_service = UserService::new(Arc::clone(&user_repo), cache.clone());
    let chat_service = ChatService::new(pool.clone(), cache.clone());
    let (chat_tx, _) = broadcast::channel(100);
    let state = AppState {
        user_repo,
        user_service,
        chat_service,
        pool: pool.clone(),
        chat_tx,
        request_metrics: RequestMetrics::default(),
        cookie_secure: config.cookie_secure,
        login_rate_limiter: rust_api_ssr::handlers::LoginRateLimiter::new(5, 900),
    };

    let app = create_router(state);

    let listener = TcpListener::bind(&config.bind_address).await?;

    info!("Listening on {}", config.bind_address);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}

async fn ensure_schema(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
