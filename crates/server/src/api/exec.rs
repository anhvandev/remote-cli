//! One-shot terminal command. No Control Lock: only input is exclusive.

use std::time::Duration;

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
use crate::audit;

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{id}/exec", post(exec))
}

const DEFAULT_TIMEOUT_SECS: u32 = 60;
const MAX_TIMEOUT_SECS: u32 = 3600;
/// Extra wait for the Agent's reply beyond the command's own timeout.
const REPLY_SLACK: Duration = Duration::from_secs(10);

/// `{"command": "...", "as": "root" (admin only; default = logged-in user), "timeout_secs": 60}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecBody {
    command: String,
    #[serde(rename = "as")]
    run_as: Option<RunAs>,
    timeout_secs: Option<u32>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RunAs {
    Root,
}

async fn exec(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    let Json(params) = body.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    let body: ExecBody = serde_json::from_value(params.clone())
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.to_string()))?;
    let timeout = body.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if !(1..=MAX_TIMEOUT_SECS).contains(&timeout) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_request", format!("timeout_secs must be 1..={MAX_TIMEOUT_SECS}")));
    }
    let as_root = body.run_as == Some(RunAs::Root);
    if as_root && !user.is_admin() {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "admin_required", "running as root/SYSTEM requires the admin role"));
    }
    let req = |request_id| {
        server_message::Msg::Exec(proto::ExecRequest {
            request_id,
            command: body.command,
            as_root,
            root_if_nobody_logged_in: user.is_admin(),
            timeout_ms: timeout * 1000,
        })
    };
    let wait = Duration::from_secs(timeout.into()) + REPLY_SLACK;
    let result = match state.agents.request_within(device, wait, req).await {
        Ok(agent_message::Msg::ExecResult(r)) => match r.error {
            Some(e) if e.code == "no_logged_in_user" => Err(ApiError::new(StatusCode::CONFLICT, "no_logged_in_user", e.message)),
            Some(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message)),
            None => Ok(json!({
                "exit_code": (!r.timed_out).then_some(r.exit_code),
                "stdout": r.stdout,
                "stderr": r.stderr,
                "timed_out": r.timed_out,
                "as": if r.as_root { "root" } else { "user" },
            })),
        },
        Ok(_) => Err(anyhow::anyhow!("unexpected reply to ExecRequest").into()),
        Err(e) => Err(e.into()),
    };
    let outcome = match &result {
        Ok(r) => json!({ "exit_code": r["exit_code"], "timed_out": r["timed_out"], "as": r["as"], "error": null }),
        Err(e) => json!({ "error": e.code }),
    };
    audit::record(&state, &user, device, "terminal", params, outcome).await?;
    result.map(Json)
}
