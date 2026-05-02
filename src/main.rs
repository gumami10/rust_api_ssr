use rust_api_ssr::app::create_router;
use rust_api_ssr::handlers::AppState;
use rust_api_ssr::models::user::SqliteUserRepository;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "rust_api_ssr=debug,axum=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    info!("Connecting to database...");

    // In-memory sqlite for demonstration, or could be a file like sqlite://data.db
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await?;

    // Create table for demo
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Insert dummy data
    sqlx::query("INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com'), ('Bob', 'bob@example.com')")
        .execute(&pool)
        .await?;

    let user_repo = Arc::new(SqliteUserRepository::new(pool));
    let state = AppState { user_repo };

    let app = create_router(state);

    let addr = "0.0.0.0:3000";
    let listener = TcpListener::bind(addr).await?;

    // Set socket options for TCP_NODELAY (usually Axum does this by default or it's accessible via Hyper, but tokio TcpListener doesn't have a direct builder for it until connection, wait, let's keep it simple and just log start)

    info!("Listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
