//! Control Lock: one holder per Device may send input Actions; anyone may observe.
//! Basic acquire/release only; idle timeout, disconnect grace and admin force-release come in ticket 07.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{auth::User, AppState};

#[derive(Serialize, Debug, Clone, sqlx::FromRow)]
pub struct Lock {
    /// User name of the holder.
    #[sqlx(rename = "holder_name")]
    pub holder: String,
    #[serde(skip)]
    pub holder_id: Uuid,
    pub acquired_at: DateTime<Utc>,
}

/// Callers pass an existing Device (the API resolves it first).
#[derive(Debug)]
pub enum Error {
    /// Someone else holds it.
    Held(Lock),
    /// Nobody holds it, but the operation needs the caller to.
    NotHeld,
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

pub async fn status(state: &AppState, device: Uuid) -> Result<Option<Lock>, Error> {
    Ok(sqlx::query_as("SELECT holder_name, holder_id, acquired_at FROM control_locks WHERE device_id = $1")
        .bind(device)
        .fetch_optional(&state.db)
        .await?)
}

/// Takes the lock if it is free; holding it already is fine. Never pre-empts another holder.
pub async fn acquire(state: &AppState, device: Uuid, user: &User) -> Result<Lock, Error> {
    sqlx::query(
        "INSERT INTO control_locks (device_id, holder_id, holder_name, acquired_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (device_id) DO NOTHING",
    )
    .bind(device)
    .bind(user.id)
    .bind(&user.name)
    .bind(state.clock.now())
    .execute(&state.db)
    .await?;
    let lock = status(state, device).await?.ok_or(Error::NotHeld)?;
    held_by(lock, user)
}

/// Releasing a free lock is a no-op; releasing someone else's is refused.
pub async fn release(state: &AppState, device: Uuid, user: &User) -> Result<(), Error> {
    if let Some(lock) = status(state, device).await? {
        held_by(lock, user)?;
        sqlx::query("DELETE FROM control_locks WHERE device_id = $1 AND holder_id = $2")
            .bind(device)
            .bind(user.id)
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

/// Ok if `user` holds the lock: the gate for input Actions.
pub async fn check(state: &AppState, device: Uuid, user: &User) -> Result<Lock, Error> {
    held_by(status(state, device).await?.ok_or(Error::NotHeld)?, user)
}

fn held_by(lock: Lock, user: &User) -> Result<Lock, Error> {
    if lock.holder_id == user.id {
        Ok(lock)
    } else {
        Err(Error::Held(lock))
    }
}
