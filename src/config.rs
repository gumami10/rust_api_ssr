#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind_address: std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data.db".into()),
        }
    }
}
