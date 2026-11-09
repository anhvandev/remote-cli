//! Interactive shell (PTY) sessions. No Control Lock: only input Actions are exclusive.
//!
//! `POST /devices/{d}/shell` answers with an NDJSON stream: `{"session","as"}`, then `{"output": base64}` lines,
//! then `{"exit_code": n}` (or `{"error": {...}}` if the Device disconnects or the Controller reads too slowly). Closing the stream ends the session.
//! Keystrokes and terminal size go to `POST /shell-sessions/{id}/input` (raw bytes) and `.../resize`.

use std::convert::Infallible;

use axum::{
    body::{Body, Bytes},
    extract::{rejection::JsonRejection, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::Engine;
use rcli_protocol::agent::{self as proto, agent_message, server_message};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{bad_request, devices::resolve_for, ApiError, Caller};
use crate::{audit, control_lock};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/shell", post(open))
        .route("/api/v1/shell-sessions/{id}/input", post(input))
        .route("/api/v1/shell-sessions/{id}/resize", post(resize))
}

/// How long the Agent may take to answer a shell open: ICE gathering and a 30 s root prompt.
const ROOT_PROMPT_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// `{"as": "root" (admin only; default = logged-in user), "cols": 80, "rows": 24}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenBody {
    #[serde(rename = "as")]
    run_as: Option<RunAs>,
    cols: Option<u32>,
    rows: Option<u32>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RunAs {
    Root,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Size {
    cols: u32,
    rows: u32,
}

/// Why a session ended, as recorded in the `shell.end` audit entry.
#[derive(Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum EndReason {
    /// The Controller closed the stream.
    Closed,
    Exited,
    DeviceDisconnected,
    /// The Controller read output too slowly; the Server dropped the session rather than buffer without bound.
    ControllerTooSlow,
}

fn check_size(cols: u32, rows: u32) -> Result<(), ApiError> {
    match (1..=1000).contains(&cols) && (1..=1000).contains(&rows) {
        true => Ok(()),
        false => Err(bad_request("cols and rows must be 1..=1000")),
    }
}

async fn open(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let device = resolve_for(&user, &state, &id).await?;
    // Terminal use by the Control Lock holder counts as an Action: restarts their idle timer.
    control_lock::touch(&state, device, &user).await?;
    let Json(params) = body.map_err(|e| bad_request(e.body_text()))?;
    let body: OpenBody = serde_json::from_value(params).map_err(bad_request)?;
    let (cols, rows) = (body.cols.unwrap_or(80), body.rows.unwrap_or(24));
    check_size(cols, rows)?;
    // Server roles no longer gate what runs on a Device: the Device's own settings do (ticket 07).
    let as_root = body.run_as == Some(RunAs::Root);
    let session = Uuid::new_v4();
    // Registered before the Agent starts the shell so no output is lost.
    let mut output = state.shells.open(session, device, user.id);
    let requester = user.name.clone();
    // The Agent may first show a root prompt for up to 30 s (ticket 07).
    let wait = std::time::Duration::from_secs(10) + ROOT_PROMPT_BUDGET;
    let req = |request_id| {
        server_message::Msg::ShellOpen(proto::ShellOpen {
            request_id,
            session: session.to_string(),
            as_root,
            cols,
            rows,
            requester: requester.clone(),
        })
    };
    let mut decision = String::new();
    let opened = match state.agents.request_within(device, wait, req).await {
        Ok(agent_message::Msg::ShellOpened(r)) => {
            decision = r.root_decision.clone();
            match r.error {
                Some(e) if e.code == "no_logged_in_user" => Err(ApiError::new(StatusCode::CONFLICT, "no_logged_in_user", e.message)),
                Some(e) if e.code == "root_denied" => Err(ApiError::new(StatusCode::FORBIDDEN, "root_denied", e.message)),
                Some(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message)),
                None => Ok(if r.as_root { "root" } else { "user" }),
            }
        }
        Ok(_) => Err(anyhow::anyhow!("unexpected reply to ShellOpen").into()),
        Err(e) => Err(e.into()),
    };
    let params = json!({ "as": if as_root { Some("root") } else { None } });
    let result = match &opened {
        Ok(who) => json!({ "session": session, "as": who, "root_decision": decision, "error": null }),
        // A refused root request is still a prompt answer worth recording.
        Err(e) => json!({ "error": e.code, "root_decision": decision }),
    };
    if let Err(e) = audit::record(&state, &user, device, "shell.start", params, result).await {
        state.shells.close(session);
        state.agents.send(device, server_message::Msg::ShellClose(proto::ShellClose { session: session.to_string() }));
        return Err(e.into());
    }
    let who = match opened {
        Ok(who) => who,
        Err(e) => {
            state.shells.close(session);
            return Err(e);
        }
    };

    // A task relays Agent output into the response body and notices when the Controller goes away.
    let (lines, lines_rx) = mpsc::channel::<Bytes>(64);
    let line = |v: Value| Bytes::from(format!("{v}\n"));
    let _ = lines.send(line(json!({ "session": session, "as": who }))).await;
    tokio::spawn(async move {
        let (exit_code, reason) = loop {
            tokio::select! {
                _ = lines.closed() => break (None, EndReason::Closed),
                msg = output.recv() => match msg {
                    Some(agent_message::Msg::ShellOutput(o)) => {
                        let data = base64::engine::general_purpose::STANDARD.encode(&o.data);
                        if lines.send(line(json!({ "output": data }))).await.is_err() {
                            break (None, EndReason::Closed);
                        }
                    }
                    Some(agent_message::Msg::ShellExit(e)) => {
                        let _ = lines.send(line(json!({ "exit_code": e.exit_code }))).await;
                        break (Some(e.exit_code), EndReason::Exited);
                    }
                    Some(_) => {}
                    None if state.shells.overflowed(session) => {
                        let error = json!({ "code": "controller_too_slow", "message": "output was not read fast enough" });
                        let _ = lines.send(line(json!({ "error": error }))).await;
                        break (None, EndReason::ControllerTooSlow);
                    }
                    None => {
                        let error = json!({ "code": "device_disconnected", "message": "the Device disconnected" });
                        let _ = lines.send(line(json!({ "error": error }))).await;
                        break (None, EndReason::DeviceDisconnected);
                    }
                },
            }
        };
        state.shells.close(session);
        if matches!(reason, EndReason::Closed | EndReason::ControllerTooSlow) {
            state.agents.send(device, server_message::Msg::ShellClose(proto::ShellClose { session: session.to_string() }));
        }
        let result = json!({ "exit_code": exit_code, "reason": reason });
        if let Err(e) = audit::record(&state, &user, device, "shell.end", json!({ "session": session }), result).await {
            tracing::error!("cannot audit shell end: {e:#}");
        }
    });
    let body = futures::stream::unfold(lines_rx, |mut rx| async move { rx.recv().await.map(|b| (Ok::<_, Infallible>(b), rx)) });
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], Body::from_stream(body)).into_response())
}

/// The Device of the caller's own live session; anyone else's (or an ended) session is 404.
fn session_device(state: &super::State, id: &str, user: Uuid) -> Result<(Uuid, Uuid), ApiError> {
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such shell session");
    let session: Uuid = id.parse().map_err(|_| not_found())?;
    state.shells.device_of(session, user).map(|d| (session, d)).ok_or_else(not_found)
}

async fn input(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    data: Bytes,
) -> Result<StatusCode, ApiError> {
    let (session, device) = session_device(&state, &id, user.id)?;
    control_lock::touch(&state, device, &user).await?;
    let msg = proto::ShellInput { session: session.to_string(), data: data.to_vec() };
    state.agents.send(device, server_message::Msg::ShellInput(msg));
    Ok(StatusCode::NO_CONTENT)
}

async fn resize(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let (session, device) = session_device(&state, &id, user.id)?;
    // Any content type: browsers send small fetch bodies as text/plain.
    let Size { cols, rows } = serde_json::from_slice(&body).map_err(bad_request)?;
    check_size(cols, rows)?;
    state.agents.send(device, server_message::Msg::ShellResize(proto::ShellResize { session: session.to_string(), cols, rows }));
    Ok(StatusCode::NO_CONTENT)
}
