//! Action Router: checks the Control Lock for input Actions, forwards to the Agent, returns the Observation.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use rcli_protocol::agent::{self as proto, action::Kind, agent_message, server_message};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{devices::resolve, ApiError, Caller};
use crate::{agent_link::RequestError, control_lock};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/actions", post(act))
        .route("/api/v1/devices/{id}/displays", get(displays))
}

/// `{"actions": [...], "observe": "shot" | "none" | "frames", "display": N | "all", "max_frames": N}`.
/// No Actions = observe only (no lock). Several Actions = an Action Batch, run in order.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActBody {
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    observe: Observe,
    /// Unset = the display last acted on.
    display: Option<DisplaySel>,
    /// `frames`: at most this many per display (default 10).
    max_frames: Option<u32>,
}

/// Upper bound of `max_frames`.
const MAX_FRAMES: u32 = 30;
/// Upper bound of all waits in one batch, so the Agent answers well within its request timeout.
const MAX_TOTAL_WAIT_MS: u64 = 5_000;

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Observe {
    #[default]
    Shot,
    None,
    Frames,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DisplaySel {
    Index(u32),
    All(AllDisplays),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AllDisplays {
    All,
}

/// A point in Observation image space of `display` (default 0).
#[derive(Deserialize)]
struct Point {
    #[serde(default)]
    display: u32,
    x: u32,
    y: u32,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Action {
    Move(Point),
    Click(Point),
    DoubleClick(Point),
    RightClick(Point),
    Drag {
        #[serde(flatten)]
        from: Point,
        to: Point,
    },
    Scroll {
        #[serde(flatten)]
        at: Point,
        #[serde(default)]
        dx: i32,
        #[serde(default)]
        dy: i32,
    },
    Type {
        text: String,
    },
    /// Key combination, e.g. "ctrl+shift+t".
    Key {
        keys: String,
    },
    /// Action Batch pause, in milliseconds.
    Wait {
        ms: u32,
    },
}

impl From<Point> for proto::Point {
    fn from(p: Point) -> Self {
        Self { display: p.display, x: p.x, y: p.y }
    }
}

impl From<Action> for proto::Action {
    fn from(a: Action) -> Self {
        let kind = match a {
            Action::Move(p) => Kind::Move(p.into()),
            Action::Click(p) => Kind::Click(p.into()),
            Action::DoubleClick(p) => Kind::DoubleClick(p.into()),
            Action::RightClick(p) => Kind::RightClick(p.into()),
            Action::Drag { from, to } => Kind::Drag(proto::Drag { from: Some(from.into()), to: Some(to.into()) }),
            Action::Scroll { at, dx, dy } => Kind::Scroll(proto::Scroll { at: Some(at.into()), dx, dy }),
            Action::Type { text } => Kind::TypeText(text),
            Action::Key { keys } => Kind::Key(keys),
            Action::Wait { ms } => Kind::WaitMs(ms),
        };
        Self { kind: Some(kind) }
    }
}

impl From<RequestError> for ApiError {
    fn from(e: RequestError) -> Self {
        match e {
            RequestError::NotConnected => ApiError::new(StatusCode::CONFLICT, "device_not_connected", "the Device's Agent is not connected"),
            RequestError::Timeout => ApiError::new(StatusCode::GATEWAY_TIMEOUT, "agent_timeout", "the Agent did not answer in time"),
        }
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_action", message)
}

fn agent_error(e: proto::Error) -> ApiError {
    match e.code.as_str() {
        "invalid_action" => ApiError::new(StatusCode::BAD_REQUEST, "invalid_action", e.message),
        _ => ApiError::new(StatusCode::BAD_GATEWAY, "agent_error", e.message),
    }
}

async fn act(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Result<Json<ActBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    let Json(body) = body.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text()))?;
    let waits: u64 = body.actions.iter().map(|a| if let Action::Wait { ms } = a { u64::from(*ms) } else { 0 }).sum();
    if waits > MAX_TOTAL_WAIT_MS {
        return Err(invalid(format!("waits add up to {waits} ms; at most {MAX_TOTAL_WAIT_MS} ms per request")));
    }
    let max_frames = body.max_frames.unwrap_or(10);
    if !(1..=MAX_FRAMES).contains(&max_frames) {
        return Err(invalid(format!("max_frames must be 1 to {MAX_FRAMES}")));
    }
    if !body.actions.is_empty() {
        control_lock::check(&state, device, &user).await?;
    }
    let req = |request_id| {
        server_message::Msg::Act(proto::ActRequest {
            request_id,
            actions: body.actions.into_iter().map(Into::into).collect(),
            observe: match body.observe {
                Observe::Shot => proto::ObserveMode::Shot,
                Observe::None => proto::ObserveMode::None,
                Observe::Frames => proto::ObserveMode::Frames,
            }
            .into(),
            display: match body.display {
                Some(DisplaySel::Index(i)) => Some(i),
                _ => None,
            },
            all_displays: matches!(body.display, Some(DisplaySel::All(_))),
            controller: user.id.to_string(),
            max_frames,
        })
    };
    let agent_message::Msg::ActResult(result) = state.agents.request(device, req).await? else {
        return Err(anyhow::anyhow!("unexpected reply to ActRequest").into());
    };
    if let Some(e) = result.error {
        let err = agent_error(e);
        return Err(match result.failed_step {
            Some(step) => err.with("step", step),
            None => err,
        });
    }
    let images: Vec<Value> = result
        .images
        .into_iter()
        .map(|i| {
            let data = (!i.unchanged).then(|| base64::engine::general_purpose::STANDARD.encode(&i.data));
            json!({ "display": i.display, "width": i.width, "height": i.height, "unchanged": i.unchanged, "mime": i.mime, "data": data, "at_ms": i.at_ms })
        })
        .collect();
    Ok(Json(json!({ "images": images })))
}

async fn displays(_: Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let device = resolve(&state, &id).await?;
    let req = |request_id| server_message::Msg::ListDisplays(proto::DisplaysRequest { request_id });
    let agent_message::Msg::Displays(list) = state.agents.request(device, req).await? else {
        return Err(anyhow::anyhow!("unexpected reply to DisplaysRequest").into());
    };
    if let Some(e) = list.error {
        return Err(agent_error(e));
    }
    let displays: Vec<Value> = list
        .displays
        .into_iter()
        .map(|d| {
            json!({ "index": d.index, "x": d.x, "y": d.y, "width": d.width, "height": d.height, "scale": d.scale, "primary": d.primary })
        })
        .collect();
    Ok(Json(json!(displays)))
}
