//! Audit Log: who did which Action on which Device, when, with what parameters and outcome.
//! Kept 90 days. Callers mask secrets (see `MASK`) before recording; media is never passed in.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{auth::User, AppState};

pub const RETENTION: chrono::Duration = chrono::Duration::days(90);
/// Replaces secret values (passwords, `unlock` input) in `params`.
pub const MASK: &str = "***";

/// Record one Action. `action` names the kind ("act", "terminal", ...); `params` must already be masked;
/// `result` is the outcome, e.g. `{"error": null}` or `{"exit_code": 0}`.
pub async fn record(state: &AppState, user: &User, device: Uuid, action: &str, params: Value, result: Value) -> anyhow::Result<()> {
    let now = state.clock.now();
    sqlx::query(
        "INSERT INTO audit_log (at, user_id, user_name, device_id, device_number, action, params, result)
         VALUES ($1, $2, $3, $4, (SELECT device_number FROM devices WHERE id = $4), $5, $6, $7)",
    )
    .bind(now)
    .bind(user.id)
    .bind(&user.name)
    .bind(device)
    .bind(action)
    .bind(params)
    .bind(result)
    .execute(&state.db)
    .await?;
    purge(state).await
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: i64,
    pub at: DateTime<Utc>,
    #[sqlx(rename = "user_name")]
    pub user: String,
    /// 9-digit Device ID at the time of the Action; null only if it predates Device IDs.
    pub device_id: Option<String>,
    /// Current Device name; null if the Device was deleted.
    pub device: Option<String>,
    pub action: String,
    pub params: Value,
    pub result: Value,
}

#[derive(Default)]
pub struct Filter {
    pub device: Option<Uuid>,
    pub user: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

/// Matching entries, oldest first. Both time bounds are inclusive.
pub async fn list(state: &AppState, f: &Filter) -> anyhow::Result<Vec<Entry>> {
    purge(state).await?;
    Ok(sqlx::query_as(
        "SELECT a.id, a.at, a.user_name, a.device_number AS device_id, d.name AS device, a.action, a.params, a.result
         FROM audit_log a LEFT JOIN devices d ON d.id = a.device_id
         WHERE ($1::uuid IS NULL OR a.device_id = $1) AND ($2::text IS NULL OR a.user_name = $2)
           AND ($3::timestamptz IS NULL OR a.at >= $3) AND ($4::timestamptz IS NULL OR a.at <= $4)
         ORDER BY a.at, a.id",
    )
    .bind(f.device)
    .bind(&f.user)
    .bind(f.since)
    .bind(f.until)
    .fetch_all(&state.db)
    .await?)
}

// shortcut: runs on every record/list, not on a timer; move to a background job if audit writes get heavy.
async fn purge(state: &AppState) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM audit_log WHERE at < $1").bind(state.clock.now() - RETENTION).execute(&state.db).await?;
    Ok(())
}
