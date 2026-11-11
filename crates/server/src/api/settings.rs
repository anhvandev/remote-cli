//! Server Settings (admin): the Server Key, the Server Address and the API Address.

use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, Admin, ApiError, Caller};
use crate::settings;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/server-settings", axum::routing::get(show))
        .route("/api/v1/server-settings/server-key/rotate", post(rotate_key))
        .route("/api/v1/server-settings/server-address", put(set_address))
        .route("/api/v1/server-settings/api-address", put(set_api_address))
}

/// Readable by every signed-in User: the Connect guide shows the Server Key and Server Address to all of them
/// (ticket 18). A password connection has no Server-admin rights and is refused (spec). Rotate and set-address
/// stay Admin.
async fn show(Caller(user): Caller, State(state): State<super::State>) -> Result<Json<Value>, ApiError> {
    if user.role == "controller" {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "user_session_required",
            "a password connection may not read the Server Settings",
        ));
    }
    Ok(Json(json!({
        "server_key": state.settings.key().await,
        "ca_fingerprint": state.ca.fingerprint,
        "server_address": state.settings.address().await,
        "api_address": state.settings.api_address().await,
    })))
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

/// An Admin's correction of the API Address: an `https://` origin, no path. Agents get it on their next connection.
async fn set_api_address(
    Admin(_): Admin,
    State(state): State<super::State>,
    body: Result<Json<Address>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let uri: axum::http::Uri = body.address.trim().parse().map_err(|_| bad_request("the API Address must be an https:// URL"))?;
    let authority = match (uri.scheme_str(), uri.authority(), uri.path_and_query().map(|p| p.as_str())) {
        (Some("https"), Some(a), None | Some("/")) if !a.as_str().contains('@') => a,
        _ => return Err(bad_request("the API Address must be an https:// origin such as https://rcli.example.com")),
    };
    let url = format!("https://{authority}");
    state.settings.set_api_address(&state.db, &url).await?;
    Ok(Json(json!({ "api_address": url })))
}
