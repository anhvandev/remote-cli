use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{devices::resolve, ApiError, SignedIn};
use crate::audit::{self, Entry};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/audit", get(list))
}

/// `?device=id-or-name&user=name&since=RFC3339&until=RFC3339` (all optional, bounds inclusive).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    device: Option<String>,
    user: Option<String>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
}

async fn list(_: SignedIn, State(state): State<super::State>, Query(p): Query<Params>) -> Result<Json<Vec<Entry>>, ApiError> {
    let device = match p.device {
        Some(d) => Some(resolve(&state, &d).await?),
        None => None,
    };
    let filter = audit::Filter { device, user: p.user, since: p.since, until: p.until };
    Ok(Json(audit::list(&state, &filter).await?))
}
