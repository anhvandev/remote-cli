//! Update Distribution: stores signed Agent builds and rolls a version out to Devices.
//! The Server never checks signatures (it has no key); Agents do, and report failures back.

use rcli_protocol::agent::{AgentHello, AgentUpdate, UpdateFailed};
use uuid::Uuid;

use crate::AppState;

/// A stored build: version, signature, sha256 (hex) and size, as served by the download endpoints.
pub type Build = (String, Vec<u8>, String, i64);

#[derive(Debug)]
pub enum UpdateError {
    BuildExists,
    UnknownVersion,
    UnknownDevice,
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for UpdateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

pub async fn publish(
    state: &AppState,
    version: &str,
    target: &str,
    signature: &[u8],
    stored: &crate::storage::Stored,
    by: &str,
) -> Result<(), UpdateError> {
    let inserted = sqlx::query(
        "INSERT INTO agent_builds (version, target, signature, sha256, size, uploaded_at, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
    )
    .bind(version)
    .bind(target)
    .bind(signature)
    .bind(&stored.sha256)
    .bind(stored.size as i64)
    .bind(state.clock.now())
    .bind(by)
    .execute(&state.db)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(UpdateError::BuildExists);
    }
    Ok(())
}

/// Targets `version` at the given Devices (`None` = every enrolled Device); returns how many.
// shortcut: "all" means the Devices enrolled now; Devices enrolled later need another rollout.
pub async fn rollout(state: &AppState, version: &str, devices: Option<&[Uuid]>) -> Result<u64, UpdateError> {
    let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM agent_builds WHERE version = $1)")
        .bind(version)
        .fetch_one(&state.db)
        .await?;
    if !known {
        return Err(UpdateError::UnknownVersion);
    }
    if let Some(ids) = devices {
        let found: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE id = ANY($1)")
            .bind(ids)
            .fetch_one(&state.db)
            .await?;
        if found as usize != ids.iter().collect::<std::collections::HashSet<_>>().len() {
            return Err(UpdateError::UnknownDevice);
        }
    }
    let n = sqlx::query(
        "UPDATE devices SET update_version = $1, update_error = NULL WHERE $2::uuid[] IS NULL OR id = ANY($2)",
    )
    .bind(version)
    .bind(devices)
    .execute(&state.db)
    .await?
    .rows_affected();
    // A rollout to every Device makes this the fleet's build: what new installs and Controller-only machines get.
    // A rollout to some Devices (a canary) does not.
    if devices.is_none() {
        sqlx::query("UPDATE agent_builds SET rolled_out_at = $2 WHERE version = $1")
            .bind(version)
            .bind(state.clock.now())
            .execute(&state.db)
            .await?;
    }
    state.rollouts.send_replace(());
    Ok(n)
}

pub async fn agent_hello(state: &AppState, device: Uuid, hello: &AgentHello) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET agent_version = $2, agent_target = $3 WHERE id = $1")
        .bind(device)
        .bind(&hello.version)
        .bind(&hello.target)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn agent_failed(state: &AppState, device: Uuid, failed: &UpdateFailed) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET update_error = $3 WHERE id = $1 AND update_version = $2")
        .bind(device)
        .bind(&failed.version)
        .bind(&failed.reason)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// The fleet's build for a target: the one last rolled out to every Device. New installs (`/download`) and
/// Controller-only machines (`rcli upgrade`) get it; a newer publish or a canary rollout does not reach them.
pub async fn fleet_build(state: &AppState, target: &str) -> anyhow::Result<Option<Build>> {
    let row: Option<Build> = sqlx::query_as(
        "SELECT version, signature, sha256, size FROM agent_builds WHERE target = $1 AND rolled_out_at IS NOT NULL
         ORDER BY rolled_out_at DESC, uploaded_at DESC LIMIT 1",
    )
    .bind(target)
    .fetch_optional(&state.db)
    .await?;
    Ok(row)
}

/// The build `rcli upgrade` fetches when it names its Device: the one rolled out to it (`update_version`), so an
/// unpublished or not-yet-rolled-out build never reaches a Device. No Device row (a Controller-only install) or no
/// rollout to it yet falls back to the fleet's build.
pub async fn upgrade_build(state: &AppState, device: Option<&str>, target: &str) -> anyhow::Result<Option<Build>> {
    let mut rolled_out = None;
    if let Some(key) = device {
        if let Ok(Some(id)) = crate::registry::resolve(state, key).await? {
            rolled_out = sqlx::query_as::<_, Build>(
                "SELECT b.version, b.signature, b.sha256, b.size
                 FROM devices d JOIN agent_builds b ON b.version = d.update_version AND b.target = $2
                 WHERE d.id = $1",
            )
            .bind(id)
            .bind(target)
            .fetch_optional(&state.db)
            .await?;
        }
    }
    match rolled_out {
        Some(row) => Ok(Some(row)),
        None => fleet_build(state, target).await,
    }
}

/// The build this Device should install now, if any. A missing build for the Device's
/// target is recorded as a failure instead.
pub async fn pending(state: &AppState, device: Uuid) -> anyhow::Result<Option<AgentUpdate>> {
    type Row = (String, String, Option<Vec<u8>>, Option<String>, Option<i64>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT d.update_version, d.agent_target, b.signature, b.sha256, b.size
         FROM devices d LEFT JOIN agent_builds b ON b.version = d.update_version AND b.target = d.agent_target
         WHERE d.id = $1 AND d.update_error IS NULL AND d.agent_target IS NOT NULL
           AND d.update_version IS DISTINCT FROM d.agent_version AND d.update_version IS NOT NULL",
    )
    .bind(device)
    .fetch_optional(&state.db)
    .await?;
    Ok(match row {
        None => None,
        Some((version, target, Some(signature), Some(sha256), Some(size))) => {
            Some(AgentUpdate { version, target, signature, size: size as u64, sha256 })
        }
        Some((version, target, ..)) => {
            let reason = format!("no build of {version} for {target}");
            agent_failed(state, device, &UpdateFailed { version, reason }).await?;
            None
        }
    })
}
