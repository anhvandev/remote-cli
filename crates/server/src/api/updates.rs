use axum::{
    body::Bytes,
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use rcli_protocol::update::MAX_BUILD_BYTES;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, devices::resolve, Admin, ApiError};
use crate::updates::{self, UpdateError};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/agent-builds/{version}/{target}", put(publish).layer(DefaultBodyLimit::max(MAX_BUILD_BYTES)))
        .route("/api/v1/agent-builds/{version}/rollout", post(rollout))
        .route("/api/v1/agent-builds/current/{target}", get(current))
}

/// Public like the update key: `rcli upgrade` fetches the newest build for its OS/arch and verifies the signature
/// against the key it pinned at setup, so serving it needs no credentials.
async fn current(State(state): State<super::State>, Path(target): Path<String>) -> Result<Response, ApiError> {
    let (version, signature, build) = updates::latest(&state, &target)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no_build", format!("no build for {target}")))?;
    Ok((
        [
            ("x-rcli-version", version),
            ("x-rcli-signature", hex::encode(signature)),
            ("content-type", "application/octet-stream".to_owned()),
        ],
        build,
    )
        .into_response())
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

/// Body: the raw build. Builds must be signed; the Server stores the signature, Agents verify it.
async fn publish(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path((version, target)): Path<(String, String)>,
    Query(q): Query<Signed>,
    build: Bytes,
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
    if build.is_empty() {
        return Err(bad_request("empty build"));
    }
    updates::publish(&state, &version, &target, &signature, &build, &user.name).await?;
    Ok((StatusCode::CREATED, Json(json!({ "version": version, "target": target, "size": build.len() }))))
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
