use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use super::{devices::resolve, Admin, ApiError, Caller};
use crate::{
    audit,
    control_lock::{self, Lock},
};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/lock", get(status).post(acquire))
        .route("/api/v1/devices/{id}/lock/release", post(release))
        .route("/api/v1/devices/{id}/lock/force-release", post(force_release))
        .route("/api/v1/devices/{id}/lock/watch", get(watch))
}

impl From<control_lock::Error> for ApiError {
    fn from(e: control_lock::Error) -> Self {
        match e {
            control_lock::Error::Held(lock) => ApiError::new(
                StatusCode::CONFLICT,
                "control_lock_held",
                format!("the Control Lock is held by {} (expires {})", lock.holder, lock.expires_at.to_rfc3339()),
            )
            .with("holder", lock.holder)
            .with("expires_at", json!(lock.expires_at)),
            control_lock::Error::NotHeld => ApiError::new(
                StatusCode::CONFLICT,
                "control_lock_required",
                "input Actions need the Control Lock; acquire it first",
            ),
            control_lock::Error::Internal(e) => e.into(),
        }
    }
}

/// `{"holder": name | null, "acquired_at": time | null, "expires_at": time | null}`.
fn body(lock: Option<Lock>) -> Json<Value> {
    Json(match lock {
        Some(l) => json!({ "holder": l.holder, "acquired_at": l.acquired_at, "expires_at": l.expires_at }),
        None => json!({ "holder": null, "acquired_at": null, "expires_at": null }),
    })
}

async fn status(_: Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(body(control_lock::status(&state, resolve(&state, &id).await?).await?))
}

async fn acquire(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(body(Some(control_lock::acquire(&state, resolve(&state, &id).await?, &user).await?)))
}

async fn release(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    control_lock::release(&state, resolve(&state, &id).await?, &user).await?;
    Ok(body(None))
}

async fn force_release(Admin(admin): Admin, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    if let Some(lock) = control_lock::force_release(&state, device).await? {
        audit::record(&state, &admin, device, "lock.force_release", json!({ "holder": lock.holder }), json!({ "error": null })).await?;
    }
    Ok(body(None))
}

/// The holder's connection: while one is open the holder counts as connected; when the last one closes,
/// the lock ends after 60 s unless the holder reconnects or acts. Streams the lock status as NDJSON
/// (the `GET .../lock` body), at once and then every 5 s, so anyone can watch it.
async fn watch(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Body, ApiError> {
    let device = resolve(&state, &id).await?;
    let conn = control_lock::connect(state.clone(), device, &user).await?;
    let lines = futures::stream::unfold((conn, state, true), move |(conn, state, first)| async move {
        if !first {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let line = match control_lock::status(&state, device).await {
            Ok(lock) => Ok(format!("{}\n", body(lock).0)),
            Err(e) => Err(std::io::Error::other(format!("{e:?}"))),
        };
        Some((line, (conn, state, false)))
    });
    Ok(Body::from_stream(lines))
}
