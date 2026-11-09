//! Update Distribution: stores signed Agent builds and rolls a version out to Devices.
//! The Server never checks signatures (it has no key); Agents do, and report failures back.

use rcli_protocol::agent::{AgentHello, AgentUpdate, UpdateFailed};
use uuid::Uuid;

use crate::AppState;

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
    build: &[u8],
    by: &str,
) -> Result<(), UpdateError> {
    let inserted = sqlx::query(
        "INSERT INTO agent_builds (version, target, signature, build, uploaded_at, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
    )
    .bind(version)
    .bind(target)
    .bind(signature)
    .bind(build)
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

/// The build `rcli upgrade` fetches for a target: the most recently published one. Rollout state is per Device, so
/// a Controller-only machine has none; the newest signed build is what an Admin made current for that OS/arch.
pub async fn latest(state: &AppState, target: &str) -> anyhow::Result<Option<(String, Vec<u8>, Vec<u8>)>> {
    let row: Option<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT version, signature, build FROM agent_builds WHERE target = $1
         ORDER BY uploaded_at DESC, version DESC LIMIT 1",
    )
    .bind(target)
    .fetch_optional(&state.db)
    .await?;
    Ok(row)
}

/// The build this Device should install now, if any. A missing build for the Device's
/// target is recorded as a failure instead.
pub async fn pending(state: &AppState, device: Uuid) -> anyhow::Result<Option<AgentUpdate>> {
    type Row = (String, String, Option<Vec<u8>>, Option<Vec<u8>>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT d.update_version, d.agent_target, b.signature, b.build
         FROM devices d LEFT JOIN agent_builds b ON b.version = d.update_version AND b.target = d.agent_target
         WHERE d.id = $1 AND d.update_error IS NULL AND d.agent_target IS NOT NULL
           AND d.update_version IS DISTINCT FROM d.agent_version AND d.update_version IS NOT NULL",
    )
    .bind(device)
    .fetch_optional(&state.db)
    .await?;
    Ok(match row {
        None => None,
        Some((version, target, Some(signature), Some(build))) => Some(AgentUpdate { version, target, build, signature }),
        Some((version, target, _, _)) => {
            let reason = format!("no build of {version} for {target}");
            agent_failed(state, device, &UpdateFailed { version, reason }).await?;
            None
        }
    })
}
