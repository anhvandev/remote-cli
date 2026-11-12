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

/// One stored Agent Build, as the Updates page lists it.
#[derive(sqlx::FromRow, serde::Serialize)]
pub struct Listed {
    pub version: String,
    pub target: String,
    pub size: i64,
    pub sha256: String,
    pub uploaded_by: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// The Latest Build for its target.
    #[sqlx(skip)]
    pub latest: bool,
}

/// Every Agent Build, newest version first (targets alphabetical within a version), the Latest Build per target marked.
pub async fn list(state: &AppState) -> anyhow::Result<Vec<Listed>> {
    let mut builds: Vec<Listed> =
        sqlx::query_as("SELECT version, target, size, sha256, uploaded_by, uploaded_at FROM agent_builds").fetch_all(&state.db).await?;
    let rank = |b: &Listed| semver::Version::parse(&b.version).ok();
    builds.sort_by(|a, b| rank(b).cmp(&rank(a)).then_with(|| a.target.cmp(&b.target)));
    let mut seen = std::collections::HashSet::new();
    for b in &mut builds {
        // The first semver row of a target is its highest version.
        b.latest = rank(b).is_some() && seen.insert(b.target.clone());
    }
    Ok(builds)
}

/// Deletes the build `version` for `target`, or every target of `version` when `target` is `None`; returns the
/// sha256 of each deleted row (empty: nothing matched).
pub async fn delete(state: &AppState, version: &str, target: Option<&str>) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar("DELETE FROM agent_builds WHERE version = $1 AND ($2::text IS NULL OR target = $2) RETURNING sha256")
        .bind(version)
        .bind(target)
        .fetch_all(&state.db)
        .await?)
}

/// The Latest Build for a target: the highest semver version (a pre-release ranks below its release). New installs
/// (`/download`), `rcli update` and Agents get it.
pub async fn latest_build(state: &AppState, target: &str) -> anyhow::Result<Option<Build>> {
    let builds: Vec<Build> =
        sqlx::query_as("SELECT version, sha256, size FROM agent_builds WHERE target = $1").bind(target).fetch_all(&state.db).await?;
    // Uploads are semver-checked; an older non-semver row is never the Latest Build.
    Ok(builds.into_iter().filter_map(|b| Some((semver::Version::parse(&b.version).ok()?, b))).max_by(|a, b| a.0.cmp(&b.0)).map(|(_, b)| b))
}
