//! Control Lock: one holder per Device may send input Actions; anyone may observe.
//! It is never pre-empted and ends on release, 10 minutes after the holder's last Action, 60 seconds after the
//! holder's connection (a `watch` stream) closed without a new one, or on admin force-release.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use rcli_protocol::agent::{server_message, ControlIndicator};
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
    /// When the lock ends unless the holder acts (or reconnects) first.
    pub expires_at: DateTime<Utc>,
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

/// Open `watch` streams per (Device, user): the holder counts as connected while it has at least one.
#[derive(Default)]
pub struct Watchers(Mutex<HashMap<(Uuid, Uuid), usize>>);

const EXPIRES_AT: &str = "LEAST(last_action_at + interval '10 minutes', disconnected_at + interval '60 seconds')";

/// Ends every lock past its idle timeout or disconnect grace and hides those Devices' indicators.
/// Runs before every lock read and every second in the background (for the indicator).
pub async fn expire(state: &AppState) -> Result<(), Error> {
    let ended: Vec<(Uuid,)> = sqlx::query_as(&format!("DELETE FROM control_locks WHERE {EXPIRES_AT} <= $1 RETURNING device_id"))
        .bind(state.clock.now())
        .fetch_all(&state.db)
        .await?;
    for (device,) in ended {
        push_indicator(state, device).await?;
    }
    Ok(())
}

pub async fn status(state: &AppState, device: Uuid) -> Result<Option<Lock>, Error> {
    expire(state).await?;
    Ok(sqlx::query_as(&format!(
        "SELECT holder_name, holder_id, acquired_at, {EXPIRES_AT} AS expires_at FROM control_locks WHERE device_id = $1"
    ))
    .bind(device)
    .fetch_optional(&state.db)
    .await?)
}

/// Takes the lock if it is free; holding it already is fine (and counts as an Action). Never pre-empts another holder.
pub async fn acquire(state: &AppState, device: Uuid, user: &User) -> Result<Lock, Error> {
    expire(state).await?;
    let now = state.clock.now();
    let taken = sqlx::query(
        "INSERT INTO control_locks (device_id, holder_id, holder_name, acquired_at, last_action_at) VALUES ($1, $2, $3, $4, $4)
         ON CONFLICT (device_id) DO NOTHING",
    )
    .bind(device)
    .bind(user.id)
    .bind(&user.name)
    .bind(now)
    .execute(&state.db)
    .await?
    .rows_affected()
        == 1;
    if taken {
        push_indicator(state, device).await?;
    } else {
        touch(state, device, user).await?;
    }
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
        push_indicator(state, device).await?;
    }
    Ok(())
}

/// Admin recovery: ends whoever's lock it is. Returns the lock that was ended, if any.
pub async fn force_release(state: &AppState, device: Uuid) -> Result<Option<Lock>, Error> {
    let lock = status(state, device).await?;
    if lock.is_some() {
        sqlx::query("DELETE FROM control_locks WHERE device_id = $1").bind(device).execute(&state.db).await?;
        push_indicator(state, device).await?;
    }
    Ok(lock)
}

/// The person at the Device disconnected `controller` (a display name): ends its lock if it held one. Returns the
/// lock that was ended, if any. `controller` may be the bare name (`alice`) or the full display (`alice
/// (Dashboard)`), so a prefix of the holder's name matches.
pub async fn release_if_holder(state: &AppState, device: Uuid, controller: &str) -> Result<Option<Lock>, Error> {
    let lock = status(state, device).await?;
    if lock.as_ref().is_some_and(|l| controller == l.holder || controller.starts_with(&l.holder)) {
        return force_release(state, device).await;
    }
    Ok(None)
}

/// An Action (any request, observe-only included) by `user`: restarts the idle timer if they hold the lock.
/// The request also proves the holder is there, so it clears a pending disconnect.
pub async fn touch(state: &AppState, device: Uuid, user: &User) -> Result<(), Error> {
    expire(state).await?;
    sqlx::query("UPDATE control_locks SET last_action_at = $3, disconnected_at = NULL WHERE device_id = $1 AND holder_id = $2")
        .bind(device)
        .bind(user.id)
        .bind(state.clock.now())
        .execute(&state.db)
        .await?;
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

/// A holder connection opened: cancels a pending disconnect. Dropping the guard closes it.
pub async fn connect(state: Arc<AppState>, device: Uuid, user: &User) -> Result<Connection, Error> {
    *state.lock_watchers.0.lock().unwrap().entry((device, user.id)).or_default() += 1;
    let conn = Connection { state: state.clone(), device, user: user.id };
    expire(&state).await?;
    sqlx::query("UPDATE control_locks SET disconnected_at = NULL WHERE device_id = $1 AND holder_id = $2")
        .bind(device)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(conn)
}

pub struct Connection {
    state: Arc<AppState>,
    device: Uuid,
    user: Uuid,
}

impl Drop for Connection {
    /// The last connection of the holder closing starts the 60 s grace.
    fn drop(&mut self) {
        let key = (self.device, self.user);
        let mut watchers = self.state.lock_watchers.0.lock().unwrap();
        let n = watchers.get_mut(&key).expect("counted on connect");
        *n -= 1;
        if *n > 0 {
            return;
        }
        watchers.remove(&key);
        let state = self.state.clone();
        drop(watchers);
        tokio::spawn(async move {
            if state.lock_watchers.0.lock().unwrap().contains_key(&key) {
                return; // reconnected meanwhile
            }
            let r = sqlx::query(
                "UPDATE control_locks SET disconnected_at = $3 WHERE device_id = $1 AND holder_id = $2 AND disconnected_at IS NULL",
            )
            .bind(key.0)
            .bind(key.1)
            .bind(state.clock.now())
            .execute(&state.db)
            .await;
            if let Err(e) = r {
                tracing::error!("cannot record Control Lock holder disconnect: {e}");
            }
        });
    }
}

/// Tells the Device whether to show its control indicator (lock held). No-op if the Agent is not connected;
/// every Device session starts with this too.
pub async fn push_indicator(state: &AppState, device: Uuid) -> Result<(), Error> {
    // Not via `status`: that would recurse into `expire`.
    let (held,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM control_locks WHERE device_id = $1)")
        .bind(device)
        .fetch_one(&state.db)
        .await?;
    // Ticket 21: also on while a live stream is active.
    state.agents.send(device, server_message::Msg::ControlIndicator(ControlIndicator { on: held }));
    Ok(())
}
