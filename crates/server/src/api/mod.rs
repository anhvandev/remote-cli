//! Controller API (HTTP). One file per area; each exposes `routes()` merged here.

mod actions;
mod audit;
mod control_lock;
mod devices;
mod exec;
mod history;
mod power;
mod live;
mod sessions;
mod settings;
mod shell;
mod updates;
mod users;
mod wake;

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;

use crate::AppState;

pub type State = Arc<AppState>;

pub fn router(state: State) -> Router {
    Router::new()
        .route("/api/v1/server-ca", get(|s: axum::extract::State<State>| async move { s.ca.cert_pem.clone() }))
        .route("/api/v1/server-key", get(server_key))
        // Public: `rcli setup` pins it over the channel the Server Key's fingerprint verifies.
        .route("/api/v1/update-key", get(update_key))
        .merge(settings::routes())
        .merge(devices::routes())
        .merge(history::routes())
        .merge(updates::routes())
        .merge(users::routes())
        .merge(control_lock::routes())
        .merge(actions::routes())
        .merge(audit::routes())
        .merge(exec::routes())
        .merge(wake::routes())
        .merge(power::routes())
        .merge(sessions::routes())
        .merge(live::routes())
        .merge(shell::routes())
        .with_state(state)
}

/// The Server Key (admin): Agents and Controllers need it to connect.
async fn server_key(
    _: Admin,
    axum::extract::State(state): axum::extract::State<State>,
) -> Json<serde_json::Value> {
    Json(json!({ "server_key": state.settings.key().await, "ca_fingerprint": state.ca.fingerprint }))
}

/// Whole seconds left until `until`, rounded up so "try again in 0s" is never shown while still locked.
fn remaining_secs(until: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> i64 {
    let ms = (until - now).num_milliseconds().max(0);
    (ms + 999) / 1000
}

/// The update-signing public key (hex), which a Controller pins at `rcli setup`; empty = no updates.
/// `mac` is its HMAC under the Server Key's secret, which the Controller already holds; an impostor rewriting
/// `/api/v1/server-ca` and `/api/v1/update-key` over plain HTTP cannot forge it.
async fn update_key(axum::extract::State(state): axum::extract::State<State>) -> Json<serde_json::Value> {
    let key = state.config.update_public_key.as_deref().map(hex::encode).unwrap_or_default();
    let mac = crate::server_key::split(&state.settings.key().await)
        .map(|(_, secret)| hex::encode(rcli_protocol::update::pin_mac(secret, &key)))
        .unwrap_or_default();
    Json(json!({ "update_key": key, "mac": mac }))
}

/// Typed API error: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    /// Extra typed fields next to code and message, e.g. `holder` of a Control Lock.
    pub details: serde_json::Map<String, serde_json::Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), details: Default::default() }
    }

    pub fn with(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = self.details;
        error.insert("code".into(), self.code.into());
        error.insert("message".into(), self.message.into());
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}

impl From<crate::agent_link::RequestError> for ApiError {
    fn from(e: crate::agent_link::RequestError) -> Self {
        use crate::agent_link::RequestError;
        match e {
            RequestError::NotConnected => ApiError::new(StatusCode::CONFLICT, "device_not_connected", "the Device's Agent is not connected"),
            RequestError::Timeout => ApiError::new(StatusCode::GATEWAY_TIMEOUT, "agent_timeout", "the Agent did not answer in time"),
        }
    }
}

/// 400 `bad_request`, e.g. a body that is not the expected JSON.
pub fn bad_request(message: impl ToString) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "bad_request", message.to_string())
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error")
    }
}

/// Extractor: the request is either a User session (bearer token) or a password connection
/// (`X-Rcli-Server-Key`, `X-Rcli-Device`, `X-Rcli-Device-Password`, `X-Rcli-Controller`).
pub struct Caller(pub crate::auth::User);

impl FromRequestParts<State> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
        let token = header("authorization").and_then(|v| v.strip_prefix("Bearer "));
        if let Some(t) = token {
            return crate::auth::authenticate(state, t)
                .await?
                .map(Caller)
                .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token"));
        }
        let (Some(server_key), Some(device), Some(password)) =
            (header("x-rcli-server-key"), header("x-rcli-device"), header("x-rcli-device-password"))
        else {
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token"));
        };
        let controller = header("x-rcli-controller").unwrap_or("unknown");
        // shortcut: this is the TCP peer, so behind a reverse proxy every Controller shares the proxy's IP and
        // bucket; trust `X-Forwarded-For` only once the proxy is a known, configured hop.
        let ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|| "127.0.0.1".into());
        crate::auth::authenticate_controller(state, server_key, device, password, controller, &ip)
            .await?
            .map(Caller)
            .map_err(|e| match e {
                crate::auth::ControllerError::Unauthorized => ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "wrong Server Key or Device Password, or no Device with this Device ID",
                ),
                crate::auth::ControllerError::Locked(until) => ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "too_many_attempts",
                    format!("too many wrong Device Passwords; try again in {}s (after {until})", remaining_secs(until, state.clock.now())),
                )
                .with("locked_until", until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            })
    }
}

/// Extractor: like `Caller`, but the user must be an admin; operators get 403 `admin_required`.
pub struct Admin(pub crate::auth::User);

impl FromRequestParts<State> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let Caller(user) = Caller::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "admin_required", "this operation requires the admin role"));
        }
        Ok(Admin(user))
    }
}
