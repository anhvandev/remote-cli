//! Controller API (HTTP). One file per area; each exposes `routes()` merged here.

mod actions;
mod audit;
mod control_lock;
mod devices;
mod exec;
mod history;
mod install;
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
        // Public. No rcli client reads it: `rcli install` pins the update key at registration over the Agent port.
        .route("/api/v1/update-key", get(update_key))
        .merge(settings::routes())
        .merge(devices::routes())
        .merge(history::routes())
        .merge(install::routes())
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

/// The update-signing public key (hex); empty = no updates. `rcli install` pins it at registration instead.
/// `mac` is its HMAC under the Server Key's secret, which the Controller already holds; an impostor rewriting
/// `/api/v1/server-ca` and `/api/v1/update-key` over plain HTTP cannot forge it.
async fn update_key(axum::extract::State(state): axum::extract::State<State>) -> Json<serde_json::Value> {
    let key = state.config.update_public_key.as_deref().map(hex::encode).unwrap_or_default();
    let mac = hex::encode(rcli_protocol::update::pin_mac(&crate::server_key::current(&state).await.secret, &key));
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

/// The Agent's typed refusal as an API error; any other code is the Agent's own failure (502 `agent_error`).
pub fn agent_error(e: rcli_protocol::agent::Error) -> ApiError {
    let (status, code) = match e.code.as_str() {
        "invalid_action" => (StatusCode::BAD_REQUEST, "invalid_action"),
        "stale_mark" => (StatusCode::BAD_REQUEST, "stale_mark"),
        "ambiguous_text" => (StatusCode::BAD_REQUEST, "ambiguous_text"),
        "text_not_found" => (StatusCode::BAD_REQUEST, "text_not_found"),
        "invalid_offer" => (StatusCode::BAD_REQUEST, "invalid_offer"),
        "no_display" => (StatusCode::CONFLICT, "no_display"),
        // The Device's root setting refused, or asked and nobody answered.
        "root_denied" => (StatusCode::FORBIDDEN, "root_denied"),
        // `password+accept` refused this Controller (rejected, timed out, or no session to ask in).
        "connect_denied" => (StatusCode::FORBIDDEN, "connect_denied"),
        _ => (StatusCode::BAD_GATEWAY, "agent_error"),
    };
    ApiError::new(status, code, e.message)
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

/// The Controller's IP: the TCP peer, or, when the peer is a trusted reverse proxy, the address that proxy appended
/// last to `X-Forwarded-For` (anything before it is the client's own word). A missing peer is a test without one.
fn client_ip(peer: Option<std::net::IpAddr>, forwarded: Option<&str>, trusted: &[std::net::IpAddr]) -> String {
    let peer = peer.unwrap_or(std::net::Ipv4Addr::LOCALHOST.into());
    let from_proxy = || forwarded?.rsplit(',').next()?.trim().parse::<std::net::IpAddr>().ok();
    match trusted.contains(&peer) {
        true => from_proxy().unwrap_or(peer).to_string(),
        false => peer.to_string(),
    }
}

/// The API Address a request reveals: the `https://` origin a trusted proxy (the Dashboard) says it arrived at.
/// `http`, localhost and private, loopback or link-local IPs never count, so a test visit on an internal port
/// cannot set it.
fn api_address_of(
    peer: Option<std::net::IpAddr>,
    proto: Option<&str>,
    host: Option<&str>,
    trusted: &[std::net::IpAddr],
) -> Option<String> {
    use std::net::IpAddr;
    if !trusted.contains(&peer?) || !proto?.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority: axum::http::uri::Authority = host?.parse().ok()?;
    let name = authority.host().trim_start_matches('[').trim_end_matches(']');
    let internal = match name.parse::<IpAddr>().map(|ip| ip.to_canonical()) {
        Ok(IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified(),
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_unspecified(),
        Err(_) => name.eq_ignore_ascii_case("localhost") || name.to_ascii_lowercase().ends_with(".localhost"),
    };
    // A `user@host` authority is not an address anyone serves the Dashboard at.
    (!internal && !authority.as_str().contains('@')).then(|| format!("https://{authority}"))
}

/// Extractor: the request is either a User session (bearer token) or a password connection
/// (`X-Rcli-Server-Key`, `X-Rcli-Device`, `X-Rcli-Device-Password`, `X-Rcli-Controller`).
pub struct Caller(pub crate::auth::User);

impl FromRequestParts<State> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
        let token = header("authorization").and_then(|v| v.strip_prefix("Bearer "));
        let peer = parts.extensions.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0.ip());
        if let Some(t) = token {
            let user = crate::auth::authenticate(state, t)
                .await?
                .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token"))?;
            let (proto, host) = (header("x-forwarded-proto"), header("x-forwarded-host"));
            if let Some(url) = user.is_admin().then(|| api_address_of(peer, proto, host, &state.config.trusted_proxies)).flatten() {
                state.settings.learn_api_address(&state.db, &url).await?;
            }
            return Ok(Caller(user));
        }
        let (Some(server_key), Some(device), Some(password)) =
            (header("x-rcli-server-key"), header("x-rcli-device"), header("x-rcli-device-password"))
        else {
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token"));
        };
        let controller = header("x-rcli-controller").unwrap_or("unknown");
        let forwarded = parts.headers.get_all("x-forwarded-for").iter().next_back().and_then(|v| v.to_str().ok());
        let ip = client_ip(peer, forwarded, &state.config.trusted_proxies);
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

#[cfg(test)]
mod tests {
    use super::client_ip;

    #[test]
    fn the_forwarded_client_ip_is_used_only_from_a_trusted_proxy() {
        let proxy = "10.0.0.2".parse().unwrap();
        let trusted = [proxy];
        // The proxy appended the real client last; the first entry is whatever the client sent.
        assert_eq!(client_ip(Some(proxy), Some("6.6.6.6, 203.0.113.7"), &trusted), "203.0.113.7");
        // Anyone else's header is ignored: a client cannot pick its own rate-limit bucket.
        assert_eq!(client_ip(Some("198.51.100.1".parse().unwrap()), Some("203.0.113.7"), &trusted), "198.51.100.1");
        assert_eq!(client_ip(Some(proxy), Some("6.6.6.6"), &[]), "10.0.0.2");
        // A trusted proxy that sent no usable header is counted as itself.
        assert_eq!(client_ip(Some(proxy), None, &trusted), "10.0.0.2");
        assert_eq!(client_ip(Some(proxy), Some("garbage"), &trusted), "10.0.0.2");
    }
}
