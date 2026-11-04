//! Controller API (HTTP). One file per area; each exposes `routes()` merged here.

mod devices;
mod enrollment;

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
        .merge(devices::routes())
        .merge(enrollment::routes())
        .with_state(state)
}

/// Typed API error: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": { "code": self.code, "message": self.message } }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error")
    }
}

/// Extractor: the request carries an admin credential.
// shortcut: only the bootstrap token from config; ticket 02 replaces this with users, roles and API tokens.
pub struct Admin;

impl FromRequestParts<State> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match token {
            Some(t) if t == state.config.bootstrap_admin_token => Ok(Admin),
            _ => Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token")),
        }
    }
}
