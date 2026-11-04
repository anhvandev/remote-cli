use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rcli_protocol::agent::{agent_message, server_message, Inventory, Process, ProcessListRequest, Resources};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{bad_request, Admin, ApiError, Caller};
use crate::{audit, metrics, registry};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices", get(list))
        .route("/api/v1/devices/{device}", get(show).patch(rename).delete(delete))
        .route("/api/v1/devices/{device}/processes", get(processes))
        .route("/api/v1/devices/{device}/revoke", post(revoke))
        .route("/api/v1/devices/{device}/merge", post(merge))
}

#[derive(Deserialize)]
struct Rename {
    name: String,
}

async fn rename(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
    body: Result<Json<Rename>, JsonRejection>,
) -> Result<Json<registry::Device>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let id = resolve(&state, &key).await?;
    registry::rename(&state, id, &body.name).await?.map_err(bad_request)?;
    audit::record(&state, &user, id, "device.rename", json!({ "name": body.name }), json!({ "error": null })).await?;
    Ok(Json(registry::get(&state, id).await?))
}

async fn delete(Admin(user): Admin, State(state): State<super::State>, Path(key): Path<String>) -> Result<StatusCode, ApiError> {
    let id = resolve(&state, &key).await?;
    let name = registry::get(&state, id).await?.name;
    registry::delete(&state, id).await?;
    // The entry outlives the Device, so it keeps the name.
    audit::record(&state, &user, id, "device.delete", json!({ "name": name }), json!({ "error": null })).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Merge {
    /// The old Device (id or name) whose history moves here; it is deleted afterwards.
    from: String,
}

async fn merge(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
    body: Result<Json<Merge>, JsonRejection>,
) -> Result<Json<registry::Device>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let new = resolve(&state, &key).await?;
    let old = resolve(&state, &body.from).await?;
    if old == new {
        return Err(bad_request("cannot merge a Device into itself"));
    }
    registry::merge(&state, old, new).await?;
    audit::record(&state, &user, new, "device.merge", json!({ "from": old }), json!({ "error": null })).await?;
    Ok(Json(registry::get(&state, new).await?))
}

async fn revoke(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<registry::Device>, ApiError> {
    let id = resolve(&state, &key).await?;
    registry::revoke(&state, id).await?;
    audit::record(&state, &user, id, "device.revoke", json!({}), json!({ "error": null })).await?;
    Ok(Json(registry::get(&state, id).await?))
}

async fn list(_: Caller, State(state): State<super::State>) -> Result<Json<Vec<registry::Device>>, ApiError> {
    Ok(Json(registry::list(&state).await?))
}

/// `{device}` is an id or a unique name.
pub(super) async fn resolve(state: &super::State, key: &str) -> Result<Uuid, ApiError> {
    match registry::resolve(state, key).await? {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(ApiError::new(StatusCode::NOT_FOUND, "device_not_found", format!("no Device {key:?}"))),
        Err(m) => Err(ApiError::new(StatusCode::CONFLICT, "ambiguous_device", m)),
    }
}

#[derive(Serialize)]
struct DeviceDetails {
    #[serde(flatten)]
    device: registry::Device,
    /// Null until the Agent has reported it.
    inventory: Option<Inventory>,
    /// Null unless the Device is Online and has reported a sample.
    resources: Option<Resources>,
}

async fn show(
    _: Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<DeviceDetails>, ApiError> {
    let id = resolve(&state, &key).await?;
    let device = registry::get(&state, id).await?;
    let resources = match device.status {
        registry::DeviceStatus::Online => metrics::current_resources(&state, id),
        _ => None,
    };
    Ok(Json(DeviceDetails { inventory: metrics::inventory(&state, id).await?, resources, device }))
}

/// Asks the Agent for its process list now; never streamed.
async fn processes(
    _: Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<Vec<Process>>, ApiError> {
    let id = resolve(&state, &key).await?;
    let req = |request_id| server_message::Msg::ListProcesses(ProcessListRequest { request_id });
    match state.agents.request(id, req).await? {
        agent_message::Msg::Processes(p) if p.error.is_empty() => Ok(Json(p.processes)),
        agent_message::Msg::Processes(p) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", p.error)),
        _ => Err(anyhow::anyhow!("unexpected reply to ListProcesses").into()),
    }
}
