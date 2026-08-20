use crate::password::verify_password;
use async_trait::async_trait;
use axum_login::{AuthUser, AuthnBackend, UserId};
use mop_core::error::AppError;
use mop_core::models::User;
use mop_db::repos::UserRepo;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::ops::Deref;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUserRecord(pub User);

impl AuthUserRecord {
    pub fn into_inner(self) -> User {
        self.0
    }
}

impl Deref for AuthUserRecord {
    type Target = User;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AuthUser for AuthUserRecord {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.0.id.clone()
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.0.password_hash.as_bytes()
    }
}

#[derive(Debug, Clone)]
pub struct MopAuthBackend {
    pool: SqlitePool,
}

impl MopAuthBackend {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl AuthnBackend for MopAuthBackend {
    type User = AuthUserRecord;
    type Credentials = Credentials;
    type Error = AppError;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let user = UserRepo::find_by_username(&self.pool, &creds.username).await?;
        let Some(user) = user else {
            return Ok(None);
        };

        if user.disabled {
            return Err(AppError::UserDisabled(user.username));
        }

        if verify_password(&creds.password, &user.password_hash)? {
            Ok(Some(AuthUserRecord(user)))
        } else {
            Ok(None)
        }
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let user = UserRepo::find_by_id(&self.pool, user_id).await?;
        Ok(user.map(AuthUserRecord))
    }
}

pub type AuthSession = axum_login::AuthSession<MopAuthBackend>;
