use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rcli_protocol::agent::{agent_message, server_message, IdentityUpdate, Inventory, Process, ProcessListRequest, Resources};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{bad_request, ApiError, Caller, SignedIn};
use crate::{audit, auth::User, metrics, registry};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices", get(list))
        .route("/api/v1/devices/{device}", get(show).patch(change).delete(delete))
        .route("/api/v1/devices/{device}/processes", get(processes))
        .route("/api/v1/devices/{device}/revoke", post(revoke))
        .route("/api/v1/devices/{device}/merge", post(merge))
}

/// An Admin's change to a Device's identity; any field may be given, each one is applied on its own.
#[derive(Deserialize)]
struct Change {
    name: Option<String>,
    device_id: Option<String>,
    device_password: Option<String>,
}

/// Renames the Device, changes its Device ID and sets a new Device Password.
/// A new ID or password is pushed to the Device's Agent first and only then stored on the Server, so a Device that
/// cannot apply it (it is Offline, or its Secret Store refuses) leaves both sides unchanged.
async fn change(
    SignedIn(user): SignedIn,
    State(state): State<super::State>,
    Path(key): Path<String>,
    body: Result<Json<Change>, JsonRejection>,
) -> Result<Json<registry::Device>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    if body.name.is_none() && body.device_id.is_none() && body.device_password.is_none() {
        return Err(bad_request("give a name, device_id or device_password"));
    }
    let id = resolve(&state, &key).await?;
    // Everything is checked before anything is pushed or stored, so a refusal leaves both sides unchanged.
    if let Some(password) = &body.device_password {
        if password.chars().count() < rcli_protocol::password::MIN_LENGTH {
            return Err(bad_request(format!(
                "the Device Password must be at least {} characters",
                rcli_protocol::password::MIN_LENGTH
            )));
        }
    }
    if let Some(number) = &body.device_id {
        match registry::validate_device_number(&state, id, number).await? {
            Ok(()) => {}
            Err(registry::NumberError::Malformed(m)) => return Err(bad_request(m)),
            Err(registry::NumberError::Taken(m)) => {
                return Err(ApiError::new(StatusCode::CONFLICT, "device_id_taken", m))
            }
        }
    }

    if let Some(password) = &body.device_password {
        apply_identity(&state, &user, id, Identity::Password(password)).await?;
    }
    if let Some(number) = &body.device_id {
        apply_identity(&state, &user, id, Identity::DeviceId(number)).await?;
    }
    if let Some(name) = &body.name {
        apply_identity(&state, &user, id, Identity::Name(name)).await?;
    }
    Ok(Json(registry::get(&state, id).await?))
}

/// One identity field an Admin changed, and how it reaches the two sides.
enum Identity<'a> {
    Password(&'a str),
    DeviceId(&'a str),
    /// The Device name belongs to the Server alone: the Agent does not push it.
    Name(&'a str),
}

impl Identity<'_> {
    /// What the Device's Agent applies first; `None` for the name.
    fn update(&self) -> Option<IdentityUpdate> {
        let mut update = IdentityUpdate::default();
        match self {
            Identity::Password(password) => update.device_password = (*password).into(),
            Identity::DeviceId(number) => update.device_id = (*number).into(),
            Identity::Name(_) => return None,
        }
        Some(update)
    }

    /// Stores the change on the Server, after the Agent applied it.
    async fn store(self, state: &super::State, id: Uuid) -> Result<(), ApiError> {
        match self {
            Identity::Password(password) => registry::set_device_password(state, id, password).await.map_err(ApiError::from),
            Identity::DeviceId(number) => registry::set_device_number(state, id, number).await?.map_err(bad_request),
            Identity::Name(name) => registry::rename(state, id, name).await?.map_err(bad_request),
        }
    }

    /// The audit action and its parameters.
    fn audit(&self) -> (&'static str, Value) {
        match self {
            Identity::Password(_) => ("device.password", json!({})),
            Identity::DeviceId(number) => ("device.id", json!({ "device_id": number })),
            Identity::Name(name) => ("device.rename", json!({ "name": name })),
        }
    }
}

/// Applies one identity change in the order that keeps both sides consistent: the Device's Agent first, then the
/// Server's row, then the audit entry, so a Device that cannot apply it leaves both sides unchanged. A fourth
/// identity field is a new `Identity` variant, not another copy of this sequence (S9).
async fn apply_identity(state: &super::State, user: &User, id: Uuid, change: Identity<'_>) -> Result<(), ApiError> {
    let (action, params) = change.audit();
    if let Some(update) = change.update() {
        push(state, id, update).await?;
    }
    change.store(state, id).await?;
    audit::record(state, user, id, action, params, json!({ "error": null })).await?;
    Ok(())
}

/// Sends an identity change to the Device's Agent and returns its refusal message, if any.
/// shortcut: a reply lost after the Agent applied the change leaves the two sides disagreeing (the Server keeps the
/// old ID/password); re-read the Device's identity on the next connection if that race ever shows up.
async fn push(state: &super::State, id: Uuid, update: IdentityUpdate) -> Result<(), ApiError> {
    let msg = |request_id| server_message::Msg::IdentityChange(IdentityUpdate { request_id, ..update });
    match state.agents.request(id, msg).await? {
        agent_message::Msg::IdentityUpdated(r) => match r.error {
            None => Ok(()),
            Some(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message)),
        },
        _ => Err(anyhow::anyhow!("unexpected reply to IdentityChange").into()),
    }
}

async fn delete(SignedIn(user): SignedIn, State(state): State<super::State>, Path(key): Path<String>) -> Result<StatusCode, ApiError> {
    let id = resolve(&state, &key).await?;
    let name = registry::get(&state, id).await?.name;
    // Recorded first: the entry captures the Device ID and name before they are gone.
    audit::record(&state, &user, id, "device.delete", json!({ "name": name }), json!({ "error": null })).await?;
    registry::delete(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Merge {
    /// The old Device (id or name) whose history moves here; it is deleted afterwards.
    from: String,
}

async fn merge(
    SignedIn(user): SignedIn,
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
    let from = registry::get(&state, old).await?.device_id;
    registry::merge(&state, old, new).await?;
    audit::record(&state, &user, new, "device.merge", json!({ "from": from }), json!({ "error": null })).await?;
    Ok(Json(registry::get(&state, new).await?))
}

async fn revoke(
    SignedIn(user): SignedIn,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<registry::Device>, ApiError> {
    let id = resolve(&state, &key).await?;
    registry::revoke(&state, id).await?;
    audit::record(&state, &user, id, "device.revoke", json!({}), json!({ "error": null })).await?;
    Ok(Json(registry::get(&state, id).await?))
}

async fn list(Caller(user): Caller, State(state): State<super::State>) -> Result<Json<Vec<registry::Device>>, ApiError> {
    let mut devices = registry::list(&state).await?;
    devices.retain(|d| user.device_scope.is_none_or(|own| own == d.id));
    Ok(Json(devices))
}

/// `{device}` is an id or a unique name.
pub(super) async fn resolve(state: &super::State, key: &str) -> Result<Uuid, ApiError> {
    match registry::resolve(state, key).await? {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(ApiError::new(StatusCode::NOT_FOUND, "device_not_found", format!("no Device {key:?}"))),
        Err(m) => Err(ApiError::new(StatusCode::CONFLICT, "ambiguous_device", m)),
    }
}

/// Like `resolve`, but a password connection may only reach its own Device.
pub(super) async fn resolve_for(user: &crate::auth::User, state: &super::State, key: &str) -> Result<Uuid, ApiError> {
    let id = resolve(state, key).await?;
    match user.device_scope {
        Some(own) if own != id => {
            Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden_device", "a password connection reaches only its own Device"))
        }
        _ => Ok(id),
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
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<DeviceDetails>, ApiError> {
    let id = resolve_for(&user, &state, &key).await?;
    let device = registry::get(&state, id).await?;
    let resources = match device.status {
        registry::DeviceStatus::Online => metrics::current_resources(&state, id),
        _ => None,
    };
    Ok(Json(DeviceDetails { inventory: metrics::inventory(&state, id).await?, resources, device }))
}

/// Asks the Agent for its process list now; never streamed.
async fn processes(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
) -> Result<Json<Vec<Process>>, ApiError> {
    let id = resolve_for(&user, &state, &key).await?;
    let req = |request_id| server_message::Msg::ListProcesses(ProcessListRequest { request_id });
    match state.agents.request(id, req).await? {
        agent_message::Msg::Processes(p) if p.error.is_empty() => Ok(Json(p.processes)),
        agent_message::Msg::Processes(p) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", p.error)),
        _ => Err(anyhow::anyhow!("unexpected reply to ListProcesses").into()),
    }
}
