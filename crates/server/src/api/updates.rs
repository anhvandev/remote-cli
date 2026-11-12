use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post, put},
    Json, Router,
};
use rcli_protocol::update::MAX_BUILD_BYTES;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, devices::resolve, Admin, ApiError};
use crate::{
    storage,
    updates::{self, UpdateError},
};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/agent-builds/{version}/{target}", put(publish).layer(DefaultBodyLimit::max(MAX_BUILD_BYTES)))
        .route("/api/v1/agent-builds/{version}/rollout", post(rollout))
        .route("/api/v1/agent-builds/{target}", get(current))
}

#[derive(Deserialize)]
struct Current {
    /// The Device asking (its 9-digit Device ID): it gets the build rolled out to it, not the newest publish.
    device: Option<String>,
}

/// `rcli upgrade` fetches the build rolled out to its Device (or, on a Controller-only install, the fleet's build)
/// and verifies the signature against the key it pinned at setup. The Server Key (header `X-Rcli-Server-Key`) is
/// the only credential, so not just anyone can ask which build a given Device ID is on.
async fn current(
    State(state): State<super::State>,
    Path(target): Path<String>,
    Query(q): Query<Current>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let presented = headers.get("x-rcli-server-key").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !crate::server_key::admits(&state, presented).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or wrong Server Key"));
    }
    let build = updates::upgrade_build(&state, q.device.as_deref(), &target).await?.ok_or_else(|| no_build(&target))?;
    build_file(&state, build, &headers).await
}

/// Streams a stored build (or the requested `Range` of it) with its version, signature and sha256 headers.
// shortcut: no cap on concurrent build downloads (a fleet-wide rollout starts them all at once); add one when fleets grow.
pub(super) async fn build_file(state: &super::State, build: updates::Build, headers: &HeaderMap) -> Result<Response, ApiError> {
    let mut response = super::stored_file(state, storage::AGENT_BUILDS, &build.sha256, build.size as u64, headers).await?;
    let h = response.headers_mut();
    h.insert("x-rcli-version", build.version.parse().map_err(anyhow::Error::from)?);
    h.insert("x-rcli-signature", hex::encode(build.signature).parse().expect("hex"));
    h.insert("x-rcli-sha256", build.sha256.parse().expect("hex"));
    Ok(response)
}

/// 404 `no_build`: nothing for `target` has been rolled out (a newer publish alone does not count).
pub(super) fn no_build(target: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "no_build",
        format!("no build for {target} is rolled out yet (an Admin runs `rcli rollout <version> --all`)"),
    )
}

impl From<UpdateError> for ApiError {
    fn from(e: UpdateError) -> Self {
        match e {
            UpdateError::BuildExists => {
                ApiError::new(StatusCode::CONFLICT, "build_exists", "a build of this version and target exists")
            }
            UpdateError::UnknownVersion => ApiError::new(StatusCode::NOT_FOUND, "not_found", "no build of this version"),
            UpdateError::UnknownDevice => ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such device"),
            UpdateError::Internal(e) => e.into(),
        }
    }
}

#[derive(Deserialize)]
struct Signed {
    /// Hex Ed25519 signature, see `rcli_protocol::update`.
    signature: Option<String>,
}

/// Body: the raw build, streamed into storage. Builds must be signed with the update key; the Server checks the
/// signature against the streamed bytes' sha256, Agents verify it again. A refused upload leaves no row and no object.
async fn publish(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path((version, target)): Path<(String, String)>,
    Query(q): Query<Signed>,
    request: Request,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let valid = |s: &str| (1..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if !valid(&version) || !valid(&target) {
        return Err(bad_request("version and target must be 1-64 chars of [A-Za-z0-9._+-]"));
    }
    let signature = q
        .signature
        .and_then(|s| hex::decode(s).ok())
        .filter(|s| s.len() == 64)
        .ok_or_else(|| bad_request("unsigned build: ?signature=<hex Ed25519 signature> is required"))?;
    // Checked before the body is read: without a key no Device could verify the build.
    let key = state.config.update_public_key.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "no_update_key",
            "the Server has no update public key configured (RCLI_UPDATE_PUBLIC_KEY), so no Device could verify this build",
        )
    })?;
    let stored = super::store(&state, storage::AGENT_BUILDS, request).await?;
    let accepted = if stored.size == 0 {
        Err(bad_request("empty build"))
    } else if !rcli_protocol::update::verify(&key, &version, &target, &stored.sha256, &signature) {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_signature",
            "the signature does not match this version, target and file",
        ))
    } else {
        updates::publish(&state, &version, &target, &signature, &stored, &user.name).await.map_err(ApiError::from)
    };
    if let Err(e) = accepted {
        // Refused, so the same version + target can be retried; the bytes stay only if another build row has them.
        super::delete_unreferenced(&state, storage::AGENT_BUILDS, &stored.sha256).await?;
        return Err(e);
    }
    Ok((StatusCode::CREATED, Json(json!({ "version": version, "target": target, "size": stored.size }))))
}

#[derive(Deserialize)]
struct Rollout {
    #[serde(default)]
    all: bool,
    /// Device IDs (9-digit), UUIDs or unique names.
    devices: Option<Vec<String>>,
}

/// Body: `{"all": true}` or `{"devices": [device, ...]}`.
async fn rollout(
    _: Admin,
    State(state): State<super::State>,
    Path(version): Path<String>,
    body: Result<Json<Rollout>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(r) = body.map_err(|e| bad_request(e.body_text()))?;
    let devices = match (r.all, r.devices) {
        (true, None) => None,
        (false, Some(keys)) if !keys.is_empty() => {
            let mut ids = Vec::with_capacity(keys.len());
            for key in &keys {
                ids.push(resolve(&state, key).await?);
            }
            Some(ids)
        }
        _ => return Err(bad_request("give either \"all\": true or a non-empty \"devices\" list")),
    };
    let n = updates::rollout(&state, &version, devices.as_deref()).await?;
    Ok(Json(json!({ "version": version, "devices": n })))
}
