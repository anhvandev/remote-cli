use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use rcli_protocol::update::MAX_BUILD_BYTES;
use serde_json::{json, Value};

use super::{bad_request, ApiError, SignedIn};
use crate::{
    storage,
    updates::{self, UpdateError},
};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/agent-builds", get(list))
        .route(
            "/api/v1/agent-builds/{version}/{target}",
            get(download).delete(delete_one).put(publish).layer(DefaultBodyLimit::max(MAX_BUILD_BYTES)),
        )
        // `{target}` on GET (the build lookup), a version on DELETE: one path, so one parameter name.
        .route("/api/v1/agent-builds/{target}", get(current).delete(delete_version))
}

/// `rcli update` and Agents fetch the Latest Build for their target and check it against `x-rcli-sha256`. The
/// Server Key (header `X-Rcli-Server-Key`) is the only credential.
async fn current(State(state): State<super::State>, Path(target): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    let presented = headers.get("x-rcli-server-key").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !crate::server_key::admits(&state, presented).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or wrong Server Key"));
    }
    let build = updates::latest_build(&state, &target).await?.ok_or_else(|| no_build(&target))?;
    build_file(&state, build, &headers).await
}

async fn list(_: SignedIn, State(state): State<super::State>) -> Result<Json<Vec<updates::Listed>>, ApiError> {
    Ok(Json(updates::list(&state).await?))
}

/// Any stored build, for a User to test or hand out.
async fn download(
    _: SignedIn,
    State(state): State<super::State>,
    Path((version, target)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let build = updates::list(&state).await?.into_iter().find(|b| b.version == version && b.target == target).ok_or_else(not_found)?;
    build_file(&state, updates::Build { version, sha256: build.sha256, size: build.size }, &headers).await
}

async fn delete_one(_: SignedIn, State(state): State<super::State>, Path((version, target)): Path<(String, String)>) -> Result<StatusCode, ApiError> {
    delete(&state, &version, Some(&target)).await
}

async fn delete_version(_: SignedIn, State(state): State<super::State>, Path(version): Path<String>) -> Result<StatusCode, ApiError> {
    delete(&state, &version, None).await
}

/// Devices running a deleted version are untouched; the stored file goes once no build row uses it.
async fn delete(state: &super::State, version: &str, target: Option<&str>) -> Result<StatusCode, ApiError> {
    let deleted = updates::delete(state, version, target).await?;
    if deleted.is_empty() {
        return Err(not_found());
    }
    for sha256 in deleted {
        super::delete_unreferenced(state, storage::AGENT_BUILDS, &sha256).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such Agent Build")
}

/// Streams a stored build (or the requested `Range` of it) with its version and sha256 headers.
// shortcut: no cap on concurrent build downloads; add one when fleets grow.
pub(super) async fn build_file(state: &super::State, build: updates::Build, headers: &HeaderMap) -> Result<Response, ApiError> {
    let mut response = super::stored_file(state, storage::AGENT_BUILDS, &build.sha256, build.size as u64, headers).await?;
    let h = response.headers_mut();
    h.insert("x-rcli-version", build.version.parse().map_err(anyhow::Error::from)?);
    h.insert("x-rcli-sha256", build.sha256.parse().expect("hex"));
    Ok(response)
}

/// 404 `no_build`: no Agent Build has been uploaded for `target`.
pub(super) fn no_build(target: &str) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "no_build", format!("no build for {target} (upload one on the Updates page)"))
}

impl From<UpdateError> for ApiError {
    fn from(e: UpdateError) -> Self {
        match e {
            UpdateError::BuildExists => ApiError::new(
                StatusCode::CONFLICT,
                "build_exists",
                "a build of this version and target exists; delete the old one first",
            ),
            UpdateError::Internal(e) => e.into(),
        }
    }
}

/// Body: the raw build, streamed into storage; the Server computes its sha256 and size. A refused upload leaves no
/// row and no object.
async fn publish(
    SignedIn(user): SignedIn,
    State(state): State<super::State>,
    Path((version, target)): Path<(String, String)>,
    request: Request,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let valid = |s: &str| (1..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if !valid(&version) || !valid(&target) {
        return Err(bad_request("version and target must be 1-64 chars of [A-Za-z0-9._+-]"));
    }
    if semver::Version::parse(&version).is_err() {
        return Err(bad_request("the version must be semver: X.Y.Z with an optional -pre-release suffix"));
    }
    let stored = super::store(&state, storage::AGENT_BUILDS, request).await?;
    let accepted = if stored.size == 0 {
        Err(bad_request("empty build"))
    } else {
        updates::publish(&state, &version, &target, &stored, &user.name).await.map_err(ApiError::from)
    };
    if let Err(e) = accepted {
        // Refused, so the same version + target can be retried; the bytes stay only if another build row has them.
        super::delete_unreferenced(&state, storage::AGENT_BUILDS, &stored.sha256).await?;
        return Err(e);
    }
    let body = json!({ "version": version, "target": target, "size": stored.size, "sha256": stored.sha256 });
    Ok((StatusCode::CREATED, Json(body)))
}
