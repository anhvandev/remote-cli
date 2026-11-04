use axum::{extract::State, routing::get, Json, Router};

use super::{ApiError, Caller};
use crate::registry;

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices", get(list))
}

async fn list(_: Caller, State(state): State<super::State>) -> Result<Json<Vec<registry::Device>>, ApiError> {
    Ok(Json(registry::list(&state).await?))
}
