//! Auth: users with a role (admin, operator) and per-user API tokens.
//! The bootstrap token from config acts as an admin only until an enabled admin has a live API token (first run).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;

pub const ROLES: [&str; 2] = ["admin", "operator"];

#[derive(Serialize, Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    /// "admin" or "operator".
    pub role: String,
    pub disabled_at: Option<DateTime<Utc>>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

#[derive(Debug)]
pub enum Error {
    BadRequest(String),
    NameTaken,
    NotFound,
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

const USER_COLS: &str = "id, name, role, disabled_at";

/// The user a bearer token belongs to, if the token is live and the user enabled.
pub async fn authenticate(state: &AppState, token: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(&format!(
        "SELECT u.{} FROM api_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND u.disabled_at IS NULL",
        USER_COLS.replace(", ", ", u.")
    ))
    .bind(hash(token))
    .fetch_optional(&state.db)
    .await?;
    if user.is_some() || token != state.config.bootstrap_admin_token {
        return Ok(user);
    }
    let admin_exists: bool =
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM api_tokens t JOIN users u ON u.id = t.user_id
             WHERE u.role = 'admin' AND u.disabled_at IS NULL AND t.revoked_at IS NULL)",
        )
        .fetch_one(&state.db)
        .await?;
    Ok((!admin_exists).then(|| User { id: Uuid::nil(), name: "bootstrap".into(), role: "admin".into(), disabled_at: None }))
}

pub async fn create_user(state: &AppState, name: &str, role: &str) -> Result<User, Error> {
    // Names go into URL paths (`/users/{name}/...`), so keep them URL-safe.
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err(Error::BadRequest("name must be 1-64 characters of A-Z a-z 0-9 . _ -".into()));
    }
    if !ROLES.contains(&role) {
        return Err(Error::BadRequest(format!("role must be one of {ROLES:?}")));
    }
    sqlx::query_as::<_, User>(&format!(
        "INSERT INTO users (id, name, role, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (name) DO NOTHING RETURNING {USER_COLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(role)
    .bind(state.clock.now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NameTaken)
}

pub async fn list_users(state: &AppState) -> anyhow::Result<Vec<User>> {
    Ok(sqlx::query_as(&format!("SELECT {USER_COLS} FROM users ORDER BY name")).fetch_all(&state.db).await?)
}

/// Disables a user; all of their tokens stop working.
pub async fn disable_user(state: &AppState, name: &str) -> Result<User, Error> {
    sqlx::query_as::<_, User>(&format!(
        "UPDATE users SET disabled_at = COALESCE(disabled_at, $2) WHERE name = $1 RETURNING {USER_COLS}"
    ))
    .bind(name)
    .bind(state.clock.now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NotFound)
}

/// Issues a new API token for `user_name`; returns `(token id, token)`. Only the hash is stored.
pub async fn issue_token(state: &AppState, user_name: &str) -> Result<(Uuid, String), Error> {
    let (id, token) = (Uuid::new_v4(), format!("rcli_{}", Uuid::new_v4().simple()));
    let inserted = sqlx::query(
        "INSERT INTO api_tokens (id, user_id, token_hash, created_at)
         SELECT $1, id, $3, $4 FROM users WHERE name = $2 AND disabled_at IS NULL",
    )
    .bind(id)
    .bind(user_name)
    .bind(hash(&token))
    .bind(state.clock.now())
    .execute(&state.db)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok((id, token))
}

pub async fn revoke_token(state: &AppState, id: Uuid) -> Result<(), Error> {
    let updated = sqlx::query("UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, $2) WHERE id = $1")
        .bind(id)
        .bind(state.clock.now())
        .execute(&state.db)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}
