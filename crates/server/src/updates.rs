//! Update Distribution: stores Agent Builds (unsigned, ADR 0008) and finds the Latest Build for a target.

use rcli_protocol::agent::{server_message, AgentHello, AgentUpdate, UpdateFailed};
use uuid::Uuid;

use crate::AppState;

/// A stored build, as served by the download endpoints.
#[derive(sqlx::FromRow)]
pub struct Build {
    pub version: String,
    /// Lowercase hex.
    pub sha256: String,
    pub size: i64,
}

#[derive(Debug)]
pub enum UpdateError {
    BuildExists,
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for UpdateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

pub async fn publish(state: &AppState, version: &str, target: &str, stored: &crate::storage::Stored, by: &str) -> Result<(), UpdateError> {
    let inserted = sqlx::query(
        "INSERT INTO agent_builds (version, target, sha256, size, uploaded_at, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
    )
    .bind(version)
    .bind(target)
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

pub async fn agent_hello(state: &AppState, device: Uuid, hello: &AgentHello) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET agent_version = $2, agent_target = $3 WHERE id = $1")
        .bind(device)
        .bind(&hello.version)
        .bind(&hello.target)
        .execute(&state.db)
        .await?;
    clear_stale_errors(state).await?;
    offer_update(state, device).await
}

/// Whether `version` is below the Minimum Version `minimum` (empty = none). A version that is not semver never is.
pub fn below(version: Option<&str>, minimum: &str) -> bool {
    match (version.map(semver::Version::parse), semver::Version::parse(minimum)) {
        (Some(Ok(v)), Ok(min)) => v < min,
        _ => false,
    }
}

pub async fn agent_failed(state: &AppState, device: Uuid, failed: &UpdateFailed) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET update_error = $2 WHERE id = $1").bind(device).bind(&failed.reason).execute(&state.db).await?;
    Ok(())
}

/// Sends the Agent the Latest Build when it is below the Minimum Version and the Latest Build is above it.
pub async fn offer_update(state: &AppState, device: Uuid) -> anyhow::Result<()> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT agent_version, agent_target FROM devices WHERE id = $1").bind(device).fetch_optional(&state.db).await?;
    let Some((Some(version), Some(target))) = row else { return Ok(()) };
    if !below(Some(&version), &state.settings.minimum_version().await) {
        return Ok(());
    }
    let Some(build) = latest_build(state, &target).await? else { return Ok(()) };
    if below(Some(&version), &build.version) {
        let update = AgentUpdate { version: build.version, target, size: build.size as u64, sha256: build.sha256 };
        state.agents.send(device, server_message::Msg::Update(update));
    }
    Ok(())
}

/// A saved Minimum Version clears errors it made stale and reaches every connected Agent.
pub async fn minimum_version_changed(state: &AppState) -> anyhow::Result<()> {
    clear_stale_errors(state).await?;
    for device in state.agents.connected() {
        offer_update(state, device).await?;
    }
    Ok(())
}

/// Clears the self-update error of every Device no longer below the Minimum Version.
async fn clear_stale_errors(state: &AppState) -> anyhow::Result<()> {
    let minimum = state.settings.minimum_version().await;
    let failed: Vec<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, agent_version FROM devices WHERE update_error IS NOT NULL").fetch_all(&state.db).await?;
    let stale: Vec<Uuid> = failed.into_iter().filter(|(_, v)| !below(v.as_deref(), &minimum)).map(|(id, _)| id).collect();
    sqlx::query("UPDATE devices SET update_error = NULL WHERE id = ANY($1)").bind(&stale).execute(&state.db).await?;
    Ok(())
}

/// The Latest Build for a target: the highest semver version (a pre-release ranks below its release). New installs
/// (`/download`), `rcli upgrade` and Agents get it.
pub async fn latest_build(state: &AppState, target: &str) -> anyhow::Result<Option<Build>> {
    let builds: Vec<Build> =
        sqlx::query_as("SELECT version, sha256, size FROM agent_builds WHERE target = $1").bind(target).fetch_all(&state.db).await?;
    // Uploads are semver-checked; an older non-semver row is never the Latest Build.
    Ok(builds.into_iter().filter_map(|b| Some((semver::Version::parse(&b.version).ok()?, b))).max_by(|a, b| a.0.cmp(&b.0)).map(|(_, b)| b))
}
