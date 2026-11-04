//! Controller API (HTTP). One file per area; each exposes `routes()` merged here.

mod actions;
mod audit;
mod control_lock;
mod devices;
mod enrollment;
mod updates;
mod users;

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
        .merge(updates::routes())
        .merge(users::routes())
        .merge(control_lock::routes())
        .merge(actions::routes())
        .merge(audit::routes())
        .with_state(state)
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

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error")
    }
}

/// Extractor: the request carries a valid API token of an enabled user (any role).
pub struct Caller(pub crate::auth::User);

impl FromRequestParts<State> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let user = match token {
            Some(t) => crate::auth::authenticate(state, t).await?,
            None => None,
        };
        user.map(Caller)
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid API token"))
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
