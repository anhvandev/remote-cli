//! Server Settings (admin): the Server Key and the Server Address.

use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, Admin, ApiError};
use crate::settings;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/server-settings", axum::routing::get(show))
        .route("/api/v1/server-settings/server-key/rotate", post(rotate_key))
        .route("/api/v1/server-settings/server-address", put(set_address))
}

async fn show(Admin(_): Admin, State(state): State<super::State>) -> Json<Value> {
    Json(json!({
        "server_key": state.settings.key().await,
        "ca_fingerprint": state.ca.fingerprint,
        "server_address": state.settings.address().await,
    }))
}

/// Changes only the secret: registered Devices keep their certificates and stay Online.
async fn rotate_key(Admin(_): Admin, State(state): State<super::State>) -> Result<Json<Value>, ApiError> {
    let server_key = state.settings.rotate_key(&state.db, &state.ca).await?;
    Ok(Json(json!({ "server_key": server_key, "ca_fingerprint": state.ca.fingerprint })))
}

#[derive(Deserialize)]
struct Address {
    address: String,
}

/// Reissues the Server's TLS certificate so new connections get the new name.
async fn set_address(
    Admin(_): Admin,
    State(state): State<super::State>,
    body: Result<Json<Address>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let hostnames = settings::hostnames(&body.address);
    if hostnames.is_empty() {
        return Err(bad_request("the Server Address must name at least one host"));
    }
    if let Err(e) = state.ca.issue_server_cert(hostnames.clone()) {
        return Err(bad_request(format!("invalid Server Address: {e}")));
    }
    state.settings.set_address(&state.db, &body.address).await?;
    *state.tls.write().await = crate::agent_link::acceptor(&state.ca, hostnames)?;
    Ok(Json(json!({ "server_address": body.address })))
}
