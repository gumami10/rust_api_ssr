use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn get_user_by_id(&self, id: i64) -> Result<Option<User>, sqlx::Error>;
    async fn list_users(&self) -> Result<Vec<User>, sqlx::Error>;
}

pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn get_user_by_id(&self, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    async fn list_users(&self) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users")
            .fetch_all(&self.pool)
            .await
    }
}
