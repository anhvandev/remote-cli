use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::{devices::resolve, ApiError, Caller};
use crate::history::{self, Point, Resolution};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{device}/history", get(show))
}

#[derive(Deserialize)]
struct Params {
    /// RFC 3339; default `to` - 1 hour.
    from: Option<DateTime<Utc>>,
    /// RFC 3339; default now.
    to: Option<DateTime<Utc>>,
    /// "5s" | "1m"; default 5s when `from` is within the raw retention, else 1m.
    resolution: Option<String>,
}

#[derive(Serialize)]
struct History {
    resolution: Resolution,
    samples: Vec<Point>,
}

async fn show(
    _: Caller,
    State(state): State<super::State>,
    Path(key): Path<String>,
    params: Result<Query<Params>, QueryRejection>,
) -> Result<Json<History>, ApiError> {
    let bad = |m: String| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", m);
    let Query(p) = params.map_err(|e| bad(e.body_text()))?;
    let id = resolve(&state, &key).await?;
    let now = state.clock.now();
    let to = p.to.unwrap_or(now);
    let from = p.from.unwrap_or(to - Duration::hours(1));
    let resolution = match p.resolution.as_deref() {
        Some("5s") => Resolution::Raw,
        Some("1m") => Resolution::Minute,
        None if from > now - history::RAW_KEEP => Resolution::Raw,
        None => Resolution::Minute,
        Some(r) => return Err(bad(format!("resolution must be 5s or 1m, not {r:?}"))),
    };
    let samples = history::query(&state, id, from, to, resolution).await?;
    Ok(Json(History { resolution, samples }))
}
