//! Server Settings (any User): the Server Key, the Server Address, the API Address and the Minimum Version.

use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, ApiError, SignedIn};
use crate::settings;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/server-settings", axum::routing::get(show))
        .route("/api/v1/server-settings/server-key/rotate", post(rotate_key))
        .route("/api/v1/server-settings/server-address", put(set_address))
        .route("/api/v1/server-settings/api-address", put(set_api_address))
        .route("/api/v1/server-settings/minimum-version", put(set_minimum_version))
}

/// Readable by every signed-in User: the Connect guide shows the Server Key and Server Address to all of them.
async fn show(_: SignedIn, State(state): State<super::State>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "server_key": state.settings.key().await,
        "ca_fingerprint": state.ca.fingerprint,
        "server_address": state.settings.address().await,
        "api_address": state.settings.api_address().await,
        "minimum_version": state.settings.minimum_version().await,
    })))
}

/// Changes only the secret: registered Devices keep their certificates and stay Online.
async fn rotate_key(_: SignedIn, State(state): State<super::State>) -> Result<Json<Value>, ApiError> {
    let server_key = state.settings.rotate_key(&state.db, &state.ca).await?;
    Ok(Json(json!({ "server_key": server_key, "ca_fingerprint": state.ca.fingerprint })))
}

#[derive(Deserialize)]
struct Address {
    address: String,
}

/// Reissues the Server's TLS certificate so new connections get the new name.
async fn set_address(
    _: SignedIn,
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

/// A User's correction of the API Address: an `https://` origin (or `http://` to loopback), no path. Agents get it
/// on their next connection.
async fn set_api_address(
    _: SignedIn,
    State(state): State<super::State>,
    body: Result<Json<Address>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let uri: axum::http::Uri = body.address.trim().parse().map_err(|_| bad_request("the API Address must be an https:// URL"))?;
    // Plain http only to this computer, the rule the CLI applies too: a Server and its Controllers on one machine.
    let (scheme, authority) = match (uri.scheme_str(), uri.authority(), uri.path_and_query().map(|p| p.as_str())) {
        (Some(scheme @ "https"), Some(a), None | Some("/")) if !a.as_str().contains('@') => (scheme, a),
        (Some(scheme @ "http"), Some(a), None | Some("/")) if !a.as_str().contains('@') && super::loopback(a.host()) => (scheme, a),
        _ => return Err(bad_request("the API Address must be an https:// origin such as https://rcli.example.com")),
    };
    let url = format!("{scheme}://{authority}");
    state.settings.set_api_address(&state.db, &url, false).await?;
    Ok(Json(json!({ "api_address": url })))
}

#[derive(Deserialize)]
struct MinimumVersion {
    version: String,
}

/// Empty clears it. Not checked against the stored Agent Builds; connected Agents below it are offered the Latest Build.
async fn set_minimum_version(
    _: SignedIn,
    State(state): State<super::State>,
    body: Result<Json<MinimumVersion>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|e| bad_request(e.body_text()))?;
    let version = body.version.trim();
    if !version.is_empty() && semver::Version::parse(version).is_err() {
        return Err(bad_request("the Minimum Version must be semver, such as 1.2.0"));
    }
    state.settings.set_minimum_version(&state.db, version).await?;
    crate::updates::minimum_version_changed(&state).await?;
    Ok(Json(json!({ "minimum_version": version })))
}
