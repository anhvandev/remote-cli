use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{bad_request, Admin, ApiError};
use crate::auth::{self, User};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/users", get(list).post(create))
        .route("/api/v1/users/{name}/disable", post(disable))
        .route("/api/v1/users/{name}/credentials", post(set_credentials))
}

impl From<auth::Error> for ApiError {
    fn from(e: auth::Error) -> Self {
        match e {
            auth::Error::BadRequest(m) => bad_request(m),
            auth::Error::NameTaken => ApiError::new(StatusCode::CONFLICT, "name_taken", "a user with this name exists"),
            auth::Error::NotFound => ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such user or token"),
            auth::Error::Internal(e) => e.into(),
        }
    }
}

#[derive(Deserialize)]
struct NewUser {
    name: String,
    role: String,
}

async fn create(
    _: Admin,
    State(state): State<super::State>,
    body: Result<Json<NewUser>, JsonRejection>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    let Json(u) = body.map_err(|e| bad_request(e.body_text()))?;
    Ok((StatusCode::CREATED, Json(auth::create_user(&state, &u.name, &u.role).await?)))
}

async fn list(_: Admin, State(state): State<super::State>) -> Result<Json<Vec<User>>, ApiError> {
    Ok(Json(auth::list_users(&state).await?))
}

async fn disable(_: Admin, State(state): State<super::State>, Path(name): Path<String>) -> Result<Json<User>, ApiError> {
    Ok(Json(auth::disable_user(&state, &name).await?))
}

#[derive(Deserialize)]
struct Credentials {
    password: String,
}

/// Sets the user's Dashboard password and enrolls a fresh TOTP secret, returned once.
async fn set_credentials(
    _: Admin,
    State(state): State<super::State>,
    Path(name): Path<String>,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(c) = body.map_err(|e| bad_request(e.body_text()))?;
    let totp = auth::set_credentials(&state, &name, &c.password).await?;
    Ok(Json(json!({ "user": name, "totp_secret": totp.secret, "otpauth_url": totp.url })))
}
