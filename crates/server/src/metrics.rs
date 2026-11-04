//! Inventory (persisted) and current resources (in memory) reported by Agents.

use std::{collections::HashMap, sync::Mutex};

use prost::Message;
use rcli_protocol::agent::{Inventory, ResourceSample, Resources};
use uuid::Uuid;

use crate::{history, AppState};

/// Latest merged resources per Device.
// shortcut: in memory only; lost on Server restart until the Agent's next sample (<= 5 s).
#[derive(Default)]
pub struct CurrentResources(Mutex<HashMap<Uuid, Resources>>);

pub async fn save_inventory(state: &AppState, device: Uuid, inventory: &Inventory) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO device_inventory (device_id, inventory, updated_at) VALUES ($1, $2, $3)
         ON CONFLICT (device_id) DO UPDATE SET inventory = $2, updated_at = $3",
    )
    .bind(device)
    .bind(inventory.encode_to_vec())
    .bind(state.clock.now())
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn inventory(state: &AppState, device: Uuid) -> anyhow::Result<Option<Inventory>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar("SELECT inventory FROM device_inventory WHERE device_id = $1")
        .bind(device)
        .fetch_optional(&state.db)
        .await?;
    Ok(bytes.map(|b| Inventory::decode(b.as_slice())).transpose()?)
}

/// A new Agent session sends a complete sample first, so forget the old one.
pub fn session_started(state: &AppState, device: Uuid) {
    state.resources.0.lock().unwrap().remove(&device);
}

/// Backfill older than this is dropped: the Agent keeps 1 hour, so anything older is bogus.
const MAX_SAMPLE_AGE: chrono::Duration = chrono::Duration::hours(2);

/// Applies a sample's delta (set fields overwrite, disk entries overwrite per mount: protobuf merge)
/// and records the merged sample in history at `now - age`.
pub async fn apply_sample(state: &AppState, device: Uuid, sample: &ResourceSample) -> anyhow::Result<()> {
    let merged = {
        let mut map = state.resources.0.lock().unwrap();
        let current = map.entry(device).or_default();
        current.merge(sample.resources.clone().unwrap_or_default().encode_to_vec().as_slice())?;
        current.clone()
    };
    let age = chrono::Duration::milliseconds(sample.age_ms.min(i64::MAX as u64) as i64);
    if age > MAX_SAMPLE_AGE {
        return Ok(());
    }
    history::record(state, device, state.clock.now() - age, &merged).await
}

pub fn current_resources(state: &AppState, device: Uuid) -> Option<Resources> {
    state.resources.0.lock().unwrap().get(&device).cloned()
}
