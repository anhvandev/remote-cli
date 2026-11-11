use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use rcli_protocol::agent::{Inventory, WakeRequest, agent_message, server_message};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ApiError, Caller, devices::resolve_for};
use crate::{audit, metrics, registry, wake};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{device}/wake", post(wake))
}

/// Wakes an Offline (or Waking) Device through a Wake Relay; -> `{relay: {id, name}, mac}`.
async fn wake(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id = resolve_for(&user, &state, &key).await?;
    let device = registry::list(&state)
        .await?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| anyhow::anyhow!("device vanished"))?;
    if device.status == registry::DeviceStatus::Online {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "device_online",
            format!("Device {:?} is Online", device.name),
        ));
    }
    let result = send(&state, id, &device.name).await;
    let outcome = match &result {
        Ok(woken) => json!({ "relay": woken["relay"]["name"], "error": null }),
        Err(e) => json!({ "relay": null, "error": e.code }),
    };
    audit::record(&state, &user, id, "wake", json!({}), outcome).await?;
    result.map(Json)
}

/// Tries every Wake Relay on the target's subnet until one sends the packet, then marks the Device Waking.
async fn send(state: &super::State, target: Uuid, name: &str) -> Result<Value, ApiError> {
    let mut relays = vec![];
    for d in registry::list(state).await? {
        if d.id != target && d.status == registry::DeviceStatus::Online {
            if let Some(inv) = metrics::inventory(state, d.id).await? {
                relays.push((d.id, d.name, inv));
            }
        }
    }
    let inventory = metrics::inventory(state, target).await?.unwrap_or_else(Inventory::default);
    let routes =
        wake::routes(name, &inventory, &relays).map_err(|m| ApiError::new(StatusCode::CONFLICT, "no_wake_relay", m))?;
    let mut failures = vec![];
    for route in routes {
        let req = |request_id| server_message::Msg::Wake(WakeRequest { request_id, mac: route.mac.to_vec() });
        match state.agents.request(route.relay, req).await {
            Ok(agent_message::Msg::WakeResult(r)) if r.error.is_none() => {
                sqlx::query("UPDATE devices SET wake_requested_at = $2 WHERE id = $1")
                    .bind(target)
                    .bind(state.clock.now())
                    .execute(&state.db)
                    .await
                    .map_err(anyhow::Error::from)?;
                let relay = json!({ "id": route.relay, "name": route.relay_name });
                return Ok(json!({ "relay": relay, "mac": wake::format_mac(route.mac) }));
            }
            Ok(agent_message::Msg::WakeResult(r)) => {
                failures.push(format!("{}: {}", route.relay_name, r.error.map(|e| e.message).unwrap_or_default()))
            }
            Ok(_) => failures.push(format!("{}: unexpected reply", route.relay_name)),
            Err(e) => failures.push(format!("{}: {e:?}", route.relay_name)),
        }
    }
    let message = format!("no Wake Relay could send the packet ({})", failures.join("; "));
    Err(ApiError::new(StatusCode::BAD_GATEWAY, "wake_relay_failed", message))
}
