use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde_json::{json, Value};

use super::{Admin, ApiError};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/enrollment-codes", post(create))
}

async fn create(_: Admin, State(state): State<super::State>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let code = crate::enrollment::create_code(&state).await?;
    // The Agent checks the CA it was given against this before enrolling (no trust on first use).
    Ok((StatusCode::CREATED, Json(json!({ "code": code, "ca_fingerprint": state.ca.fingerprint }))))
}
