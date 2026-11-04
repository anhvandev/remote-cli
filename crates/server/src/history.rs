//! Resource history: raw 5-second samples kept 7 days, 1-minute rollups kept 90 days.
//! Rollup and retention run in `maintain`: every minute, and before each history query (so tests drive it with the clock).

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, DurationRound, Utc};
use rcli_protocol::agent::Resources;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;

pub const RAW_KEEP: Duration = Duration::days(7);
pub const ROLLUP_KEEP: Duration = Duration::days(90);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Resolution {
    #[serde(rename = "5s")]
    Raw,
    #[serde(rename = "1m")]
    Minute,
}

#[derive(Serialize)]
pub struct Point {
    /// Sample time, or the start of the minute for rollups.
    pub at: DateTime<Utc>,
    pub resources: Value,
}

/// Serializes `maintain` runs (single Server process) so concurrent rollups of a minute cannot interleave.
#[derive(Default)]
pub struct Maintenance(pub tokio::sync::Mutex<()>);

/// Stores one complete sample.
pub async fn record(state: &AppState, device: Uuid, at: DateTime<Utc>, resources: &Resources) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO resource_samples (device_id, at, resources) VALUES ($1, $2, $3)")
        .bind(device)
        .bind(at)
        .bind(serde_json::to_value(resources)?)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Points of `device` with `from <= at <= to`, oldest first.
pub async fn query(
    state: &AppState,
    device: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    resolution: Resolution,
) -> anyhow::Result<Vec<Point>> {
    maintain(state).await?;
    let sql = match resolution {
        Resolution::Raw => {
            "SELECT at, resources FROM resource_samples WHERE device_id = $1 AND at >= $2 AND at <= $3 ORDER BY at"
        }
        Resolution::Minute => {
            "SELECT minute, resources FROM resource_rollups
             WHERE device_id = $1 AND minute >= $2 AND minute <= $3 ORDER BY minute"
        }
    };
    let rows: Vec<(DateTime<Utc>, Value)> =
        sqlx::query_as(sql).bind(device).bind(from).bind(to).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|(at, resources)| Point { at, resources }).collect())
}

/// Rolls up every complete minute with new samples (including backfilled ones), then applies retention.
pub async fn maintain(state: &AppState) -> anyhow::Result<()> {
    let _running = state.maintenance.0.lock().await;
    let now = state.clock.now();
    let current_minute = now.duration_trunc(Duration::minutes(1))?;
    // Marked before reading, so a sample arriving meanwhile stays pending and its minute is redone next time.
    let pending: BTreeSet<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "UPDATE resource_samples SET rolled_up = true WHERE NOT rolled_up AND at < $1
         RETURNING device_id, date_trunc('minute', at, 'UTC')",
    )
    .bind(current_minute)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();
    for (device, minute) in pending {
        let samples: Vec<Value> = sqlx::query_scalar(
            "SELECT resources FROM resource_samples WHERE device_id = $1 AND at >= $2 AND at < $3 ORDER BY at",
        )
        .bind(device)
        .bind(minute)
        .bind(minute + Duration::minutes(1))
        .fetch_all(&state.db)
        .await?;
        sqlx::query(
            "INSERT INTO resource_rollups (device_id, minute, resources) VALUES ($1, $2, $3)
             ON CONFLICT (device_id, minute) DO UPDATE SET resources = EXCLUDED.resources",
        )
        .bind(device)
        .bind(minute)
        .bind(rollup(&samples.iter().collect::<Vec<_>>()))
        .execute(&state.db)
        .await?;
    }
    sqlx::query("DELETE FROM resource_samples WHERE at <= $1").bind(now - RAW_KEEP).execute(&state.db).await?;
    sqlx::query("DELETE FROM resource_rollups WHERE minute <= $1").bind(now - ROLLUP_KEEP).execute(&state.db).await?;
    Ok(())
}

/// Combines samples (oldest first): numbers are averaged, objects (e.g. disks by mount) per key,
/// anything else (session, user, battery_charging) takes the last value. Nulls (unavailable) are ignored.
fn rollup(values: &[&Value]) -> Value {
    let present: Vec<&Value> = values.iter().copied().filter(|v| !v.is_null()).collect();
    if !present.is_empty() && present.iter().all(|v| v.is_number()) {
        let sum: f64 = present.iter().filter_map(|v| v.as_f64()).sum();
        return serde_json::json!(sum / present.len() as f64);
    }
    match present.last() {
        Some(Value::Object(_)) => {
            let keys: BTreeSet<&String> = present.iter().filter_map(|v| v.as_object()).flat_map(|o| o.keys()).collect();
            let field = |k: &String| rollup(&present.iter().map(|v| &v[k]).collect::<Vec<_>>());
            Value::Object(keys.into_iter().map(|k| (k.clone(), field(k))).collect())
        }
        Some(v) => (*v).clone(),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn rollup_averages_numbers_per_key_and_keeps_the_last_other_value() {
        let a = json!({"cpu_percent": 10.0, "disk_used_percent": {"C:": 40.0}, "session": "Active", "gpu_percent": null});
        let b = json!({"cpu_percent": 20.0, "disk_used_percent": {"C:": 50.0, "D:": 1.0}, "session": "LockScreen", "gpu_percent": 6.0});
        assert_eq!(
            super::rollup(&[&a, &b]),
            json!({"cpu_percent": 15.0, "disk_used_percent": {"C:": 45.0, "D:": 1.0}, "session": "LockScreen", "gpu_percent": 6.0})
        );
    }
}
