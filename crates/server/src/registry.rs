//! Device Registry: Devices and Device Status.

use std::{collections::HashMap, sync::Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::AppState;

/// A Device stays Online this long after its connection drops.
pub const OFFLINE_AFTER: Duration = Duration::seconds(30);

/// A woken Device has this long to come Online before the Wake counts as failed.
pub const WAKE_TIMEOUT: Duration = Duration::minutes(3);

/// Live Agent connections per Device (a count, so an overlapping reconnect is not a drop).
#[derive(Default)]
pub struct Connections(Mutex<HashMap<Uuid, usize>>);

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    Online,
    Offline,
    Waking,
}

#[derive(Serialize, Debug)]
pub struct Device {
    pub id: Uuid,
    /// 9-digit Device ID; the CLI and Dashboard address a Device by it.
    pub device_id: String,
    pub name: String,
    pub status: DeviceStatus,
    /// When the Agent's last connection ended; null while connected or never connected.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// "Wake failed" when the last Wake did not bring the Device Online in time; else null.
    pub wake_error: Option<String>,
    /// Version the Agent reported on its last connection; null until it connected once.
    pub agent_version: Option<String>,
    /// The Agent update rolled out to this Device, if any.
    pub update: Option<Update>,
    /// When an admin revoked the Device's credential; a revoked Device can no longer connect.
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Debug)]
pub struct Update {
    pub version: String,
    /// "pending" | "updated" | "failed"
    pub status: &'static str,
    /// Why the Device could not install `version` (when failed).
    pub error: Option<String>,
}

fn update(agent_version: Option<&str>, version: Option<String>, error: Option<String>) -> Option<Update> {
    let version = version?;
    let status = match (&error, agent_version == Some(version.as_str())) {
        (Some(_), _) => "failed",
        (None, true) => "updated",
        (None, false) => "pending",
    };
    Some(Update { version, status, error })
}

fn status(
    connected: bool,
    last_seen_at: Option<DateTime<Utc>>,
    woken_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> DeviceStatus {
    match (connected, last_seen_at, woken_at) {
        (true, _, _) => DeviceStatus::Online,
        (false, Some(t), _) if now - t < OFFLINE_AFTER => DeviceStatus::Online,
        (false, _, Some(w)) if now - w < WAKE_TIMEOUT => DeviceStatus::Waking,
        _ => DeviceStatus::Offline,
    }
}

pub async fn list(state: &AppState) -> anyhow::Result<Vec<Device>> {
    fetch(state, None).await
}

pub async fn get(state: &AppState, id: Uuid) -> anyhow::Result<Device> {
    fetch(state, Some(id)).await?.pop().ok_or_else(|| anyhow::anyhow!("device {id} vanished"))
}

/// All Devices, or just `only`.
async fn fetch(state: &AppState, only: Option<Uuid>) -> anyhow::Result<Vec<Device>> {
    type Row = (
        Uuid,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, device_number, name, last_seen_at, agent_version, update_version, update_error, revoked_at, wake_requested_at
         FROM devices WHERE $1::uuid IS NULL OR id = $1 ORDER BY enrolled_at, id",
    )
    .bind(only)
    .fetch_all(&state.db)
    .await?;
    let now = state.clock.now();
    let connected = state.connections.0.lock().unwrap();
    Ok(rows
        .into_iter()
        .map(|(id, device_id, name, last_seen_at, agent_version, update_version, update_error, revoked_at, woken_at)| {
            let status = status(connected.contains_key(&id), last_seen_at, woken_at, now);
            let wake_failed = woken_at.is_some() && status == DeviceStatus::Offline;
            Device {
                id,
                device_id,
                name,
                status,
                last_seen_at,
                wake_error: wake_failed.then(|| "Wake failed".to_owned()),
                update: update(agent_version.as_deref(), update_version, update_error),
                agent_version,
                revoked_at,
            }
        })
        .collect())
}

/// Finds a Device by its 9-digit Device ID, its UUID or its name. `Ok(None)` if none;
/// an error message if the key is not unique (a name shared by several Devices).
pub async fn resolve(state: &AppState, key: &str) -> anyhow::Result<Result<Option<Uuid>, String>> {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM devices WHERE device_number = $1 OR id::text = $1 OR name = $1 LIMIT 2")
            .bind(key)
            .fetch_all(&state.db)
            .await?;
    Ok(match ids[..] {
        [] => Ok(None),
        [id] => Ok(Some(id)),
        _ => Err(format!("more than one Device matches {key:?}; use its Device ID")),
    })
}

/// The Device holding this certificate, unless it was revoked.
pub async fn find_by_cert_fingerprint(state: &AppState, fp: &[u8]) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM devices WHERE cert_fingerprint = $1 AND revoked_at IS NULL")
        .bind(fp)
        .fetch_optional(&state.db)
        .await?)
}

/// Revokes the Device's credential and drops its connection. Revoking again keeps the first time.
pub async fn revoke(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET revoked_at = COALESCE(revoked_at, $2) WHERE id = $1")
        .bind(id)
        .bind(state.clock.now())
        .execute(&state.db)
        .await?;
    state.agents.disconnect(id, "credential_revoked", "this Device's credential was revoked");
    Ok(())
}

/// Why a new Device ID was refused: not 9 digits, or another Device has it.
pub enum NumberError {
    Malformed(String),
    Taken(String),
}

impl NumberError {
    pub fn message(&self) -> &str {
        match self {
            NumberError::Malformed(m) | NumberError::Taken(m) => m,
        }
    }
}

/// Checks a new Device ID without changing anything; `Err` with a reason if it is not 9 digits or another
/// Device has it. Used before a change is pushed to the Device, so a refusal changes nothing.
pub async fn validate_device_number(state: &AppState, id: Uuid, number: &str) -> anyhow::Result<Result<(), NumberError>> {
    if number.len() != 9 || !number.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Err(NumberError::Malformed("the Device ID must be 9 digits".into())));
    }
    let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM devices WHERE device_number = $1 AND id <> $2)")
        .bind(number)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if taken {
        return Ok(Err(NumberError::Taken(format!("Device ID {number} is already used by another Device"))));
    }
    Ok(Ok(()))
}

/// Changes the Device ID; `Err` with a message if it is not 9 digits or another Device has it.
pub async fn set_device_number(state: &AppState, id: Uuid, number: &str) -> anyhow::Result<Result<(), String>> {
    if let Err(e) = validate_device_number(state, id, number).await? {
        return Ok(Err(e.message().to_owned()));
    }
    sqlx::query("UPDATE devices SET device_number = $2 WHERE id = $1").bind(id).bind(number).execute(&state.db).await?;
    Ok(Ok(()))
}

/// Stores a new Device Password hash; existing connections are unaffected (they were authenticated already).
pub async fn set_device_password(state: &AppState, id: Uuid, password: &str) -> anyhow::Result<()> {
    let hash = rcli_protocol::password::hash(password);
    sqlx::query("UPDATE devices SET device_password_hash = $2 WHERE id = $1").bind(id).bind(hash).execute(&state.db).await?;
    Ok(())
}

/// Renames the Device; `Err` with a message if the name is not 1-255 characters.
pub async fn rename(state: &AppState, id: Uuid, name: &str) -> anyhow::Result<Result<(), String>> {
    let name = name.trim();
    if name.is_empty() || name.len() > 255 {
        return Ok(Err("name must be 1-255 characters".into()));
    }
    sqlx::query("UPDATE devices SET name = $2 WHERE id = $1").bind(id).bind(name).execute(&state.db).await?;
    Ok(Ok(()))
}

/// Deletes the Device with its history and drops its connection; its certificate no longer gets in.
pub async fn delete(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM devices WHERE id = $1").bind(id).execute(&state.db).await?;
    state.agents.disconnect(id, "device_deleted", "this Device was deleted");
    Ok(())
}

/// Moves `old`'s resource history and audit entries to `new` (where both have a rollup for the same minute, `new`'s wins),
/// then deletes `old`. Never done automatically: an admin decides two Devices are the same machine.
pub async fn merge(state: &AppState, old: Uuid, new: Uuid) -> anyhow::Result<()> {
    state.agents.disconnect(old, "device_deleted", "this Device was merged into another");
    // No rollup may run while samples change owner.
    let _maintenance = state.maintenance.0.lock().await;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE resource_samples SET device_id = $2 WHERE device_id = $1")
        .bind(old)
        .bind(new)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE resource_rollups o SET device_id = $2 WHERE device_id = $1
         AND NOT EXISTS (SELECT 1 FROM resource_rollups n WHERE n.device_id = $2 AND n.minute = o.minute)",
    )
    .bind(old)
    .bind(new)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE audit_log SET device_id = $2,
             device_number = (SELECT device_number FROM devices WHERE id = $2)
         WHERE device_id = $1",
    )
    .bind(old)
    .bind(new)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM devices WHERE id = $1").bind(old).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn agent_connected(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    *state.connections.0.lock().unwrap().entry(id).or_default() += 1;
    sqlx::query("UPDATE devices SET last_seen_at = NULL, wake_requested_at = NULL WHERE id = $1").bind(id).execute(&state.db).await?;
    Ok(())
}

pub async fn agent_disconnected(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    let last = {
        let mut map = state.connections.0.lock().unwrap();
        let n = map.entry(id).or_default();
        *n = n.saturating_sub(1);
        let last = *n == 0;
        if last {
            map.remove(&id);
        }
        last
    };
    if last {
        sqlx::query("UPDATE devices SET last_seen_at = $2 WHERE id = $1")
            .bind(id)
            .bind(state.clock.now())
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_for_30_seconds_after_drop_then_offline() {
        let t = Utc::now();
        assert_eq!(status(true, None, None, t), DeviceStatus::Online);
        assert_eq!(status(false, None, None, t), DeviceStatus::Offline);
        assert_eq!(status(false, Some(t), None, t + Duration::seconds(29)), DeviceStatus::Online);
        assert_eq!(status(false, Some(t), None, t + Duration::seconds(30)), DeviceStatus::Offline);
    }

    #[test]
    fn waking_for_3_minutes_after_a_wake_then_offline() {
        let t = Utc::now();
        assert_eq!(status(false, Some(t), Some(t), t + Duration::seconds(179)), DeviceStatus::Waking);
        assert_eq!(status(false, Some(t), Some(t), t + Duration::seconds(180)), DeviceStatus::Offline);
    }
}
