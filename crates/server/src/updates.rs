//! Update Distribution: stores Agent Builds (unsigned, ADR 0008) and finds the Latest Build for a target.

use rcli_protocol::agent::AgentHello;
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
