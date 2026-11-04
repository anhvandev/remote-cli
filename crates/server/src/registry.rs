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
    type Row =
        (Uuid, String, Option<DateTime<Utc>>, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, name, last_seen_at, agent_version, update_version, update_error, wake_requested_at
         FROM devices ORDER BY enrolled_at, id",
    )
    .fetch_all(&state.db)
    .await?;
    let now = state.clock.now();
    let connected = state.connections.0.lock().unwrap();
    Ok(rows
        .into_iter()
        .map(|(id, name, last_seen_at, agent_version, update_version, update_error, woken_at)| {
            let status = status(connected.contains_key(&id), last_seen_at, woken_at, now);
            let wake_failed = woken_at.is_some() && status == DeviceStatus::Offline;
            Device {
                id,
                name,
                status,
                last_seen_at,
                wake_error: wake_failed.then(|| "Wake failed".to_owned()),
                update: update(agent_version.as_deref(), update_version, update_error),
                agent_version,
            }
        })
        .collect())
}

/// Finds a Device by id or by name. `Ok(None)` if none; an error message if the name is ambiguous.
pub async fn resolve(state: &AppState, key: &str) -> anyhow::Result<Result<Option<Uuid>, String>> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM devices WHERE id::text = $1 OR name = $1 LIMIT 2")
        .bind(key)
        .fetch_all(&state.db)
        .await?;
    Ok(match ids[..] {
        [] => Ok(None),
        [id] => Ok(Some(id)),
        _ => Err(format!("more than one Device is named {key:?}; use its id")),
    })
}

pub async fn find_by_cert_fingerprint(state: &AppState, fp: &[u8]) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM devices WHERE cert_fingerprint = $1")
        .bind(fp)
        .fetch_optional(&state.db)
        .await?)
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
