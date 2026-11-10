//! One-shot terminal command. No Control Lock: only input is exclusive.

use std::time::Duration;

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::post,
    Json, Router,
};
use rcli_protocol::agent::{self as proto, agent_message, server_message};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{agent_error, bad_request, devices::resolve_for, ApiError, Caller};
use crate::{agent_link::PROMPT_BUDGET, audit, control_lock};

pub fn routes() -> Router<super::State> {
    Router::new().route("/api/v1/devices/{id}/exec", post(exec))
}

const DEFAULT_TIMEOUT_SECS: u32 = 60;
const MAX_TIMEOUT_SECS: u32 = 3600;
/// Extra wait for the Agent's reply beyond the command's own timeout.
const REPLY_SLACK: Duration = Duration::from_secs(10);

/// `{"command": "...", "as": "root" (the Device's root setting decides; default = its Terminal User), "timeout_secs": 60}`.
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
    // The Agent may first show an accept prompt and a root prompt, up to 30 s each.
    let wait = Duration::from_secs(timeout.into()) + REPLY_SLACK + 2 * PROMPT_BUDGET;
    // How root was decided matters for the audit even when the Device refused.
    let mut decision = String::new();
    let result = match state.agents.request_within(device, wait, req).await {
        Ok(agent_message::Msg::ExecResult(r)) => {
            decision = r.root_decision.clone();
            match r.error {
                Some(e) => Err(agent_error(e)),
                None => Ok(json!({
                    "exit_code": (!r.timed_out).then_some(r.exit_code),
                    "stdout": r.stdout,
                    "stderr": r.stderr,
                    "timed_out": r.timed_out,
                    "as": if r.as_root { "root" } else { "user" },
                    "root_decision": r.root_decision,
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
            "error": null,
        }),
        Err(e) => json!({ "error": e.code, "root_decision": decision }),
    };
    audit::record(&state, &user, device, "terminal", params, outcome).await?;
    result.map(Json)
}
