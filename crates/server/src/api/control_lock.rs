use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use super::{device_id, ApiError, Caller};
use crate::control_lock::{self, Lock};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/lock", get(status).post(acquire))
        .route("/api/v1/devices/{id}/lock/release", post(release))
}

impl From<control_lock::Error> for ApiError {
    fn from(e: control_lock::Error) -> Self {
        match e {
            control_lock::Error::NoSuchDevice => ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such device"),
            control_lock::Error::Held(lock) => ApiError::new(
                StatusCode::CONFLICT,
                "control_lock_held",
                format!("the Control Lock is held by {}", lock.holder),
            )
            .with("holder", lock.holder),
            control_lock::Error::NotHeld => ApiError::new(
                StatusCode::CONFLICT,
                "control_lock_required",
                "input Actions need the Control Lock; acquire it first",
            ),
            control_lock::Error::Internal(e) => e.into(),
        }
    }
}

/// `{"holder": name | null, "acquired_at": time | null}`.
fn body(lock: Option<Lock>) -> Json<Value> {
    Json(match lock {
        Some(l) => json!({ "holder": l.holder, "acquired_at": l.acquired_at }),
        None => json!({ "holder": null, "acquired_at": null }),
    })
}

async fn status(_: Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(body(control_lock::status(&state, device_id(&id)?).await?))
}

async fn acquire(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(body(Some(control_lock::acquire(&state, device_id(&id)?, &user).await?)))
}

async fn release(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    control_lock::release(&state, device_id(&id)?, &user).await?;
    Ok(body(None))
}
