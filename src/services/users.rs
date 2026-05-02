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
}

impl UserService {
    pub fn new(user_repo: Arc<dyn UserRepository + Send + Sync>) -> Self {
        Self { user_repo }
    }

    pub async fn list_users(&self) -> Result<Vec<User>, UserServiceError> {
        Ok(self.user_repo.list_users().await?)
    }

    pub async fn get_user(&self, id: i64) -> Result<Option<User>, UserServiceError> {
        Ok(self.user_repo.get_user_by_id(id).await?)
    }

    pub async fn create_user(&self, user: NewUser) -> Result<User, UserServiceError> {
        if self
            .user_repo
            .get_user_by_email(&user.email)
            .await?
            .is_some()
        {
            return Err(UserServiceError::DuplicateEmail);
        }

        let hashed_password =
            hash_password(&user.password).map_err(|err| UserServiceError::PasswordHash(err.to_string()))?;
        Ok(self
            .user_repo
            .create_user(NewUser {
                name: user.name,
                email: user.email,
                password: hashed_password,
            })
            .await?)
    }

    pub async fn update_user(
        &self,
        id: i64,
        user: UpdateUser,
    ) -> Result<Option<User>, UserServiceError> {
        let current_user = match self.user_repo.get_user_by_id(id).await? {
            Some(user) => user,
            None => return Ok(None),
        };

        if current_user.email != user.email
            && self
                .user_repo
                .get_user_by_email(&user.email)
                .await?
                .is_some()
        {
            return Err(UserServiceError::DuplicateEmail);
        }

        Ok(self.user_repo.update_user(id, user).await?)
    }

    pub async fn delete_user(&self, id: i64) -> Result<bool, UserServiceError> {
        Ok(self.user_repo.delete_user(id).await?)
    }

    pub async fn authenticate_user(
        &self,
        email: &str,
        password: &str,
    ) -> Result<User, UserServiceError> {
        let user = self
            .user_repo
            .get_user_with_password_by_email(email)
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
        })
    }
}

pub fn hash_password(password: &str) -> Result<String, password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default().hash_password(password.as_bytes(), &salt)?;
    Ok(hashed.to_string())
}
