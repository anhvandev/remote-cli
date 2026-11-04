use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use rcli_protocol::agent::{agent_message, server_message, Inventory, Process, ProcessListRequest, Resources};
use serde::Serialize;
use uuid::Uuid;

use super::{ApiError, Caller};
use crate::{agent_link::RequestError, metrics, registry};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices", get(list))
        .route("/api/v1/devices/{device}", get(show))
        .route("/api/v1/devices/{device}/processes", get(processes))
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
