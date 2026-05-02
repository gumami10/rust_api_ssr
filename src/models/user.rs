use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct UpdateUser {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserWithPassword {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub password_hash: String,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn get_user_by_id(&self, id: i64) -> Result<Option<User>, sqlx::Error>;
    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error>;
    async fn get_user_with_password_by_email(
        &self,
        email: &str,
    ) -> Result<Option<UserWithPassword>, sqlx::Error>;
    async fn list_users(&self) -> Result<Vec<User>, sqlx::Error>;
    async fn create_user(&self, user: NewUser) -> Result<User, sqlx::Error>;
    async fn update_user(&self, id: i64, user: UpdateUser) -> Result<Option<User>, sqlx::Error>;
    async fn delete_user(&self, id: i64) -> Result<bool, sqlx::Error>;
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

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }

    async fn get_user_with_password_by_email(
        &self,
        email: &str,
    ) -> Result<Option<UserWithPassword>, sqlx::Error> {
        sqlx::query_as::<_, UserWithPassword>(
            "SELECT id, name, email, password_hash FROM users WHERE email = ?",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
    }

    async fn list_users(&self) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users")
            .fetch_all(&self.pool)
            .await
    }

    async fn create_user(&self, user: NewUser) -> Result<User, sqlx::Error> {
        let id = sqlx::query("INSERT INTO users (name, email, password_hash) VALUES (?, ?, ?)")
            .bind(user.name)
            .bind(user.email)
            .bind(user.password)
            .execute(&self.pool)
            .await?
            .last_insert_rowid();

        self.get_user_by_id(id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    async fn update_user(&self, id: i64, user: UpdateUser) -> Result<Option<User>, sqlx::Error> {
        let result = sqlx::query("UPDATE users SET name = ?, email = ? WHERE id = ?")
            .bind(user.name)
            .bind(user.email)
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.get_user_by_id(id).await
    }

    async fn delete_user(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}
