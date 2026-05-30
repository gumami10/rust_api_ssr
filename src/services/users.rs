use crate::cache::AppCache;
use crate::context::QueryContext;
use crate::models::user::{NewUser, UpdateUser, User, UserRepository};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use password_hash::{rand_core::OsRng, SaltString};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum UserServiceError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Email already exists")]
    DuplicateEmail,
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Password hashing error: {0}")]
    PasswordHash(String),
}

#[derive(Clone)]
pub struct UserService {
    user_repo: Arc<dyn UserRepository + Send + Sync>,
    cache: AppCache,
}

impl UserService {
    pub fn new(user_repo: Arc<dyn UserRepository + Send + Sync>, cache: AppCache) -> Self {
        Self { user_repo, cache }
    }

    pub async fn list_users(&self, ctx: QueryContext) -> Result<Vec<User>, UserServiceError> {
        Ok(self.user_repo.list_users(ctx).await?)
    }

    pub async fn get_user(
        &self,
        ctx: QueryContext,
        id: i64,
    ) -> Result<Option<User>, UserServiceError> {
        Ok(self.user_repo.get_user_by_id(ctx, id).await?)
    }

    pub async fn create_user(&self, user: NewUser) -> Result<User, UserServiceError> {
        if self
            .user_repo
            .get_user_by_email(QueryContext::default(), &user.email)
            .await?
            .is_some()
        {
            return Err(UserServiceError::DuplicateEmail);
        }

        let hashed_password =
            hash_password(&user.password).map_err(|err| UserServiceError::PasswordHash(err.to_string()))?;

        let nickname = user.nickname.and_then(|n| {
            let trimmed = n.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(generate_nickname(&trimmed))
            }
        });

        Ok(self
            .user_repo
            .create_user(NewUser {
                name: user.name,
                email: user.email,
                password: hashed_password,
                nickname,
            })
            .await?)
    }

    pub async fn update_user(
        &self,
        id: i64,
        user: UpdateUser,
    ) -> Result<Option<User>, UserServiceError> {
        let current_user = match self.user_repo.get_user_by_id(QueryContext::default(), id).await? {
            Some(user) => user,
            None => return Ok(None),
        };

        if current_user.email != user.email
            && self
                .user_repo
                .get_user_by_email(QueryContext::default(), &user.email)
                .await?
                .is_some()
        {
            return Err(UserServiceError::DuplicateEmail);
        }

        let nickname = user.nickname.and_then(|n| {
            let trimmed = n.trim().to_string();
            if trimmed.is_empty() {
                None
            } else if current_user.nickname.as_deref() == Some(&trimmed) {
                Some(trimmed)
            } else {
                Some(generate_nickname(&trimmed))
            }
        });

        Ok(self.user_repo.update_user(id, UpdateUser {
            name: user.name,
            email: user.email,
            nickname,
        }).await?)
    }

    pub async fn delete_user(&self, id: i64) -> Result<bool, UserServiceError> {
        Ok(self.user_repo.delete_user(id).await?)
    }

    pub async fn authenticate_user(
        &self,
        ctx: QueryContext,
        email: &str,
        password: &str,
    ) -> Result<User, UserServiceError> {
        let user = self
            .user_repo
            .get_user_with_password_by_email(ctx, email)
            .await?
            .ok_or(UserServiceError::InvalidCredentials)?;

        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|err| UserServiceError::PasswordHash(err.to_string()))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| UserServiceError::InvalidCredentials)?;

        Ok(User {
            id: user.id,
            name: user.name,
            email: user.email,
            nickname: user.nickname,
        })
    }

    pub async fn validate_session(
        &self,
        ctx: QueryContext,
        token: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<Option<User>, UserServiceError> {
        if !ctx.bypass_cache {
            if let Some(user) = self.cache.session_by_token.get(token).await {
                return Ok(user);
            }
        }

        let user = sqlx::query_as::<_, User>(
            r#"
            SELECT users.id, users.name, users.email, users.nickname
            FROM sessions
            INNER JOIN users ON users.id = sessions.user_id
            WHERE sessions.token = ?
              AND sessions.created_at > datetime('now', '-30 days')
            "#,
        )
        .bind(token)
        .fetch_optional(pool)
        .await
        .map_err(UserServiceError::Database)?;

        self.cache.session_by_token.insert(token.to_string(), user.clone()).await;
        Ok(user)
    }

    pub async fn invalidate_session(&self, token: &str) {
        self.cache.session_by_token.invalidate(token).await;
    }

    pub async fn get_users_by_ids(
        &self,
        ctx: QueryContext,
        ids: &[i64],
        pool: &sqlx::SqlitePool,
    ) -> Result<Vec<User>, UserServiceError> {
        let mut users = Vec::with_capacity(ids.len());
        let mut missing = Vec::new();

        for id in ids {
            if !ctx.bypass_cache {
                if let Some(user) = self.cache.user_by_id.get(id).await {
                    users.push(user);
                    continue;
                }
            }
            missing.push(*id);
        }

        if !missing.is_empty() {
            let placeholders = missing.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let query = format!("SELECT id, name, email, nickname FROM users WHERE id IN ({})", placeholders);
            let mut request = sqlx::query_as::<_, User>(&query);

            for id in &missing {
                request = request.bind(id);
            }

            let fetched = request.fetch_all(pool).await.map_err(UserServiceError::Database)?;
            for user in &fetched {
                self.cache.user_by_id.insert(user.id, user.clone()).await;
            }
            users.extend(fetched);
        }

        Ok(users)
    }
}

pub fn hash_password(password: &str) -> Result<String, password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default().hash_password(password.as_bytes(), &salt)?;
    Ok(hashed.to_string())
}

pub fn generate_nickname(base: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let random = nanos % 10000;
    format!("{}#{:04}", base, random)
}
