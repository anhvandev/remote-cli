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

use super::{bad_request, devices::resolve_for, ApiError, Caller};
use crate::{audit, control_lock};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{id}/exec", post(exec))
}

const DEFAULT_TIMEOUT_SECS: u32 = 60;
const MAX_TIMEOUT_SECS: u32 = 3600;
/// Extra wait for the Agent's reply beyond the command's own timeout.
/// The Agent may first show a root prompt for up to 30 s (ticket 07), so allow that on top.
const REPLY_SLACK: Duration = Duration::from_secs(10);
const ROOT_PROMPT_BUDGET: Duration = Duration::from_secs(30);

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
    let device = resolve_for(&user, &state, &id).await?;
    // A command from the Control Lock holder counts as an Action: restarts their idle timer.
    control_lock::touch(&state, device, &user).await?;
    let Json(params) = body.map_err(|e| bad_request(e.body_text()))?;
    let body: ExecBody = serde_json::from_value(params.clone())
        .map_err(|e| bad_request(e.to_string()))?;
    let timeout = body.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if !(1..=MAX_TIMEOUT_SECS).contains(&timeout) {
        return Err(bad_request(format!("timeout_secs must be 1..={MAX_TIMEOUT_SECS}")));
    }
    // Server roles no longer gate what runs on a Device: the Device's own settings do (ticket 07).
    let as_root = body.run_as == Some(RunAs::Root);
    // The Device names who asks in a root prompt; a password connection already reads `<host> (<IP>)`.
    let requester = user.name.clone();
    let controller = user.controller_display();
    let req = |request_id| {
        server_message::Msg::Exec(proto::ExecRequest {
            request_id,
            command: body.command,
            as_root,
            timeout_ms: timeout * 1000,
            requester: requester.clone(),
            controller: controller.clone(),
        })
    };
    let wait = Duration::from_secs(timeout.into()) + REPLY_SLACK + ROOT_PROMPT_BUDGET;
    // How root and the connection were decided matters for the audit even when the Device refused.
    let mut decision = String::new();
    let mut connect = String::new();
    let result = match state.agents.request_within(device, wait, req).await {
        Ok(agent_message::Msg::ExecResult(r)) => {
            decision = r.root_decision.clone();
            connect = r.connect_decision.clone();
            match r.error {
                Some(e) if e.code == "no_logged_in_user" => Err(ApiError::new(StatusCode::CONFLICT, "no_logged_in_user", e.message)),
                // The Device's root setting refused, or asked and nobody answered: a plain refusal to the caller.
                Some(e) if e.code == "root_denied" => Err(ApiError::new(StatusCode::FORBIDDEN, "root_denied", e.message)),
                // `password+accept` refused this new connection (rejected, timed out, or no session to ask in).
                Some(e) if e.code == "connect_denied" => Err(ApiError::new(StatusCode::FORBIDDEN, "connect_denied", e.message)),
                Some(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message)),
                None => Ok(json!({
                    "exit_code": (!r.timed_out).then_some(r.exit_code),
                    "stdout": r.stdout,
                    "stderr": r.stderr,
                    "timed_out": r.timed_out,
                    "as": if r.as_root { "root" } else { "user" },
                    "root_decision": r.root_decision,
                    "connect_decision": r.connect_decision,
                })),
            }
        }
        Ok(_) => Err(anyhow::anyhow!("unexpected reply to ExecRequest").into()),
        Err(e) => Err(e.into()),
    };
    let outcome = match &result {
        Ok(r) => json!({
            "exit_code": r["exit_code"],
            "timed_out": r["timed_out"],
            "as": r["as"],
            "root_decision": r["root_decision"],
            "connect_decision": r["connect_decision"],
            "error": null,
        }),
        Err(e) => json!({ "error": e.code, "root_decision": decision, "connect_decision": connect }),
    };
    audit::record(&state, &user, device, "terminal", params, outcome).await?;
    result.map(Json)
}
