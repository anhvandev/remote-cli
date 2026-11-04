use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rcli_protocol::agent::{agent_message, server_message, Inventory, Process, ProcessListRequest, Resources};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Admin, ApiError, Caller};
use crate::{agent_link::RequestError, metrics, registry};

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
    _: Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
    body: Result<Json<Rename>, JsonRejection>,
) -> Result<Json<registry::Device>, ApiError> {
    let Json(body) = body.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    let id = resolve(&state, &key).await?;
    registry::rename(&state, id, &body.name).await?.map_err(|m| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", m))?;
    Ok(Json(registry::get(&state, id).await?))
}

async fn delete(_: Admin, State(state): State<super::State>, Path(key): Path<String>) -> Result<StatusCode, ApiError> {
    let id = resolve(&state, &key).await?;
    registry::delete(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Merge {
    /// The old Device (id or name) whose history moves here; it is deleted afterwards.
    from: String,
}

async fn merge(
    _: Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
    body: Result<Json<Merge>, JsonRejection>,
) -> Result<Json<registry::Device>, ApiError> {
    let Json(body) = body.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    let new = resolve(&state, &key).await?;
    let old = resolve(&state, &body.from).await?;
    if old == new {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_request", "cannot merge a Device into itself"));
    }
    registry::merge(&state, old, new).await?;
    Ok(Json(registry::get(&state, new).await?))
}

async fn revoke(
    _: Admin,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<registry::Device>, ApiError> {
    let id = resolve(&state, &key).await?;
    registry::revoke(&state, id).await?;
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
    let device = registry::list(&state)
        .await?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| anyhow::anyhow!("device vanished"))?;
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
    match state.agents.request(id, req).await {
        Ok(agent_message::Msg::Processes(p)) if p.error.is_empty() => Ok(Json(p.processes)),
        Ok(agent_message::Msg::Processes(p)) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", p.error)),
        Ok(_) => Err(anyhow::anyhow!("unexpected reply to ListProcesses").into()),
        Err(RequestError::NotConnected) => {
            Err(ApiError::new(StatusCode::CONFLICT, "device_not_connected", "the Device's Agent is not connected"))
        }
        Err(RequestError::Timeout) => {
            Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "agent_timeout", "the Agent did not answer in time"))
        }
    }
}
