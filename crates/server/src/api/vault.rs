//! Secrets Vault admin routes. Responses never contain the password.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{devices::resolve, Admin, ApiError};
use crate::vault;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/unlock-password", get(status).post(set))
        .route("/api/v1/devices/{id}/unlock-password/clear", post(clear))
}

impl From<vault::Error> for ApiError {
    fn from(e: vault::Error) -> Self {
        match e {
            vault::Error::NoMasterKey => ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "vault_unavailable",
                "the Server has no RCLI_MASTER_KEY, so unlock passwords are disabled",
            ),
            vault::Error::NotSet => ApiError::new(
                StatusCode::CONFLICT,
                "unlock_password_not_set",
                "no unlock password is stored for this Device; an admin sets it with `rcli unlock-password set`",
            ),
            vault::Error::Internal(e) => e.into(),
        }
    }
}

#[derive(Deserialize)]
struct SetBody {
    password: String,
}

async fn status(_: Admin, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    body(vault::updated_at(&state, resolve(&state, &id).await?).await?)
}

/// `{"set": bool, "updated_at": time | null}`.
fn body(updated_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "set": updated_at.is_some(), "updated_at": updated_at })))
}

async fn set(
    _: Admin,
    State(state): State<super::State>,
    Path(id): Path<String>,
    req: Result<Json<SetBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    let Json(req) = req.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    if req.password.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_request", "password must not be empty"));
    }
    vault::set(&state, device, &req.password).await?;
    body(vault::updated_at(&state, device).await?)
}

async fn clear(_: Admin, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    vault::clear(&state, resolve(&state, &id).await?).await?;
    body(None)
}
