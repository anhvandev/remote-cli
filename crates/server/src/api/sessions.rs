//! Dashboard sign-in (password + TOTP) and the signed-in user.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, ApiError, Caller};
use crate::auth::{self, User};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/sessions", post(sign_in))
        .route("/api/v1/session", delete(sign_out))
        .route("/api/v1/me", get(me))
}

#[derive(Deserialize)]
struct SignIn {
    name: String,
    password: String,
    totp: String,
}

async fn sign_in(
    State(state): State<super::State>,
    body: Result<Json<SignIn>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(b) = body.map_err(|e| bad_request(e.body_text()))?;
    let (user, token) = auth::sign_in(&state, &b.name, &b.password, b.totp.trim()).await?.map_err(|e| match e {
        auth::SignInError::Invalid => {
            ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials", "wrong name, password or TOTP code")
        }
        auth::SignInError::Locked(until) => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            format!("too many failed sign-ins; try again after {until}"),
        )
        .with("locked_until", until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    })?;
    let expires_at = state.clock.now() + auth::SESSION_TTL;
    Ok((StatusCode::CREATED, Json(json!({ "token": token, "user": user, "expires_at": expires_at }))))
}

async fn sign_out(_: Caller, State(state): State<super::State>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    auth::revoke_by_token(&state, token.unwrap_or_default()).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(Caller(user): Caller) -> Json<User> {
    Json(user)
}
