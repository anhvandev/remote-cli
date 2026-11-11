//! Live view: WebRTC signaling between a viewer and a Device's Agent. Media never passes through the Server.
//! Watching needs no Control Lock.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rcli_protocol::agent::{agent_message, server_message, IceServer, LiveClose, LiveOffer};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{devices::resolve_for, ApiError, Caller};
use crate::audit;

/// How long minted TURN credentials stay valid; a live view must start within it (running streams keep going).
const TURN_CREDENTIALS_TTL: Duration = Duration::from_secs(12 * 3600);

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/ice-servers", get(ice_servers))
        .route("/api/v1/devices/{device}/live", post(open))
        .route("/api/v1/devices/{device}/live/{session}", delete(close))
        .route("/api/v1/devices/{device}/media", post(open_media))
        .route("/api/v1/devices/{device}/media/{session}", delete(close))
}

/// STUN/TURN servers for a peer connection, with fresh TURN credentials in coturn's REST API format:
/// username = expiry unix time, credential = base64(HMAC-SHA1(secret, username)).
fn ice(state: &super::State) -> Vec<IceServer> {
    let config = &state.config;
    if config.ice_urls.is_empty() {
        return vec![];
    }
    let (username, credential) = match &config.turn_secret {
        // Wall clock, not `state.clock`: coturn checks the expiry against its own real time.
        Some(secret) => {
            let expiry = (SystemTime::now() + TURN_CREDENTIALS_TTL).duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
            let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret.as_bytes());
            let mac = STANDARD.encode(ring::hmac::sign(&key, expiry.as_bytes()));
            (expiry, mac)
        }
        None => Default::default(),
    };
    vec![IceServer { urls: config.ice_urls.clone(), username, credential }]
}

async fn ice_servers(_: Caller, State(state): State<super::State>) -> Json<Value> {
    let servers: Vec<Value> =
        ice(&state).into_iter().map(|s| json!({"urls": s.urls, "username": s.username, "credential": s.credential})).collect();
    Json(servers.into())
}

#[derive(Deserialize)]
struct Offer {
    sdp: String,
    display: Option<u32>,
}

async fn open(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
    Json(offer): Json<Offer>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let device = resolve_for(&user, &state, &key).await?;
    let params = json!({ "display": offer.display });
    let reply = signal_for(&user, &state, &key, offer, false).await;
    let result = match &reply {
        Ok((_, Json(body))) => json!({ "session": body["session"], "error": null }),
        Err(e) => json!({ "error": e.code }),
    };
    audit::record(&state, &user, device, "live.start", params, result).await?;
    reply
}

/// A media channel: a data channel that carries large Observations and clips of `.../actions` requests
/// naming its session (`media_session`). Closed like a live view (`DELETE .../media/{session}`).
async fn open_media(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
    Json(offer): Json<Offer>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    signal_for(&user, &state, &key, offer, true).await
}

/// Relays the peer's SDP offer (all ICE candidates included) to the Agent and returns its answer.
async fn signal_for(
    user: &crate::auth::User,
    state: &super::State,
    key: &str,
    offer: Offer,
    media: bool,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let id = resolve_for(user, state, key).await?;
    let session = uuid::Uuid::new_v4().to_string();
    let msg = |request_id| {
        let (ice_servers, controller) = (ice(state), user.controller_display());
        server_message::Msg::LiveOffer(LiveOffer { request_id, session: session.clone(), sdp: offer.sdp, ice_servers, display: offer.display, media, controller })
    };
    match state.agents.request_admitted(id, msg).await? {
        agent_message::Msg::LiveAnswer(a) => match a.error {
            None => Ok((StatusCode::CREATED, Json(json!({ "session": session, "sdp": a.sdp })))),
            Some(e) => Err(super::agent_error(e)),
        },
        _ => Err(anyhow::anyhow!("unexpected reply to LiveOffer").into()),
    }
}

/// Stops a live stream. Idempotent: unknown sessions and disconnected Devices are fine.
async fn close(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path((key, session)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = resolve_for(&user, &state, &key).await?;
    state.agents.send(id, server_message::Msg::LiveClose(LiveClose { session }));
    Ok(StatusCode::NO_CONTENT)
}
