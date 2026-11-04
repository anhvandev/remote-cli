//! Power Actions. Restart/Shutdown/Sleep respect the Control Lock (holder or admin); Lock screen needs none.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use rcli_protocol::agent::{self as proto, agent_message, server_message};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{devices::resolve, ApiError, Caller};
use crate::{audit, control_lock};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{id}/power", post(power))
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Action {
    Restart,
    Shutdown,
    Sleep,
    Lock,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PowerBody {
    action: Action,
}

/// `{"action": "restart" | "shutdown" | "sleep" | "lock"}` -> `{"action": ...}` once the Agent has done it.
async fn power(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    control_lock::touch(&state, device, &user).await?;
    let Json(params) = body.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    let body: PowerBody = serde_json::from_value(params.clone())
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.to_string()))?;
    if body.action != Action::Lock {
        match control_lock::status(&state, device).await? {
            Some(l) if l.holder_id != user.id && !user.is_admin() => return Err(control_lock::Error::Held(l).into()),
            _ => {}
        }
    }
    let (name, action) = match body.action {
        Action::Restart => ("restart", proto::PowerAction::Restart),
        Action::Shutdown => ("shutdown", proto::PowerAction::Shutdown),
        Action::Sleep => ("sleep", proto::PowerAction::Sleep),
        Action::Lock => ("lock", proto::PowerAction::LockScreen),
    };
    let req = |request_id| server_message::Msg::Power(proto::PowerRequest { request_id, action: action as i32 });
    let result = match state.agents.request(device, req).await {
        Ok(agent_message::Msg::PowerResult(r)) => match r.error {
            Some(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message)),
            None => Ok(json!({ "action": name })),
        },
        Ok(_) => Err(anyhow::anyhow!("unexpected reply to PowerRequest").into()),
        Err(e) => Err(e.into()),
    };
    let outcome = json!({ "error": result.as_ref().err().map(|e| e.code) });
    audit::record(&state, &user, device, &format!("power.{name}"), json!({}), outcome).await?;
    result.map(Json)
}
