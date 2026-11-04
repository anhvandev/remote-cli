//! Device Registry: Devices and Device Status.

use std::{collections::HashMap, sync::Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::AppState;

/// A Device stays Online this long after its connection drops.
pub const OFFLINE_AFTER: Duration = Duration::seconds(30);

/// Live Agent connections per Device (a count, so an overlapping reconnect is not a drop).
#[derive(Default)]
pub struct Connections(Mutex<HashMap<Uuid, usize>>);

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    Online,
    Offline,
}

#[derive(Serialize, Debug)]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub status: DeviceStatus,
    /// When the Agent's last connection ended; null while connected or never connected.
    pub last_seen_at: Option<DateTime<Utc>>,
}

fn status(connected: bool, last_seen_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DeviceStatus {
    match (connected, last_seen_at) {
        (true, _) => DeviceStatus::Online,
        (false, Some(t)) if now - t < OFFLINE_AFTER => DeviceStatus::Online,
        _ => DeviceStatus::Offline,
    }
}

pub async fn list(state: &AppState) -> anyhow::Result<Vec<Device>> {
    let rows: Vec<(Uuid, String, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT id, name, last_seen_at FROM devices ORDER BY enrolled_at, id")
            .fetch_all(&state.db)
            .await?;
    let now = state.clock.now();
    let connected = state.connections.0.lock().unwrap();
    Ok(rows
        .into_iter()
        .map(|(id, name, last_seen_at)| Device {
            id,
            name,
            status: status(connected.contains_key(&id), last_seen_at, now),
            last_seen_at,
        })
        .collect())
}

pub async fn exists(state: &AppState, id: Uuid) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM devices WHERE id = $1)").bind(id).fetch_one(&state.db).await?)
}

pub async fn find_by_cert_fingerprint(state: &AppState, fp: &[u8]) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM devices WHERE cert_fingerprint = $1")
        .bind(fp)
        .fetch_optional(&state.db)
        .await?)
}

pub async fn agent_connected(state: &AppState, id: Uuid) -> anyhow::Result<()> {
    *state.connections.0.lock().unwrap().entry(id).or_default() += 1;
    sqlx::query("UPDATE devices SET last_seen_at = NULL WHERE id = $1").bind(id).execute(&state.db).await?;
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
        assert_eq!(status(true, None, t), DeviceStatus::Online);
        assert_eq!(status(false, None, t), DeviceStatus::Offline);
        assert_eq!(status(false, Some(t), t + Duration::seconds(29)), DeviceStatus::Online);
        assert_eq!(status(false, Some(t), t + Duration::seconds(30)), DeviceStatus::Offline);
    }
}
