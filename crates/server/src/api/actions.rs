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

/// `{"actions": [...], "observe": "shot" | "none", "display": N | "all"}`. No Actions = observe only (no lock).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActBody {
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    observe: Observe,
    /// Unset = the display last acted on.
    display: Option<DisplaySel>,
    /// Detect Marks: annotated images plus `observation` id and `marks` table in the reply.
    #[serde(default)]
    marks: bool,
}

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Observe {
    #[default]
    Shot,
    None,
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

/// A target on `display` (default 0): a point in Observation image space, a Mark number from Observation
/// `observation`, or a Mark's visible text.
#[derive(Deserialize)]
struct Point {
    #[serde(default)]
    display: u32,
    #[serde(flatten)]
    target: Target,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Target {
    Xy { x: u32, y: u32 },
    Mark { mark: u32, observation: u64 },
    Text { text: String },
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
    /// Types `text`, first clicking `into` if given.
    Type {
        text: String,
        into: Option<Point>,
    },
    /// Key combination, e.g. "ctrl+shift+t".
    Key {
        keys: String,
    },
}

impl From<Point> for proto::Point {
    fn from(p: Point) -> Self {
        let display = p.display;
        match p.target {
            Target::Xy { x, y } => Self { display, x, y, ..Default::default() },
            Target::Mark { mark, observation } => Self { display, mark: Some(mark), observation, ..Default::default() },
            Target::Text { text } => Self { display, text: Some(text), ..Default::default() },
        }
    }
}

impl Action {
    /// The protocol Actions this one becomes (typing into a target is a click, then the text).
    fn into_proto(self) -> Vec<proto::Action> {
        let kind = match self {
            Action::Type { text, into: Some(at) } => {
                return [Kind::Click(at.into()), Kind::TypeText(text)].map(|k| proto::Action { kind: Some(k) }).to_vec()
            }
            Action::Move(p) => Kind::Move(p.into()),
            Action::Click(p) => Kind::Click(p.into()),
            Action::DoubleClick(p) => Kind::DoubleClick(p.into()),
            Action::RightClick(p) => Kind::RightClick(p.into()),
            Action::Drag { from, to } => Kind::Drag(proto::Drag { from: Some(from.into()), to: Some(to.into()) }),
            Action::Scroll { at, dx, dy } => Kind::Scroll(proto::Scroll { at: Some(at.into()), dx, dy }),
            Action::Type { text, into: None } => Kind::TypeText(text),
            Action::Key { keys } => Kind::Key(keys),
        };
        vec![proto::Action { kind: Some(kind) }]
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

fn agent_error(e: proto::Error) -> ApiError {
    let bad_request = |code| ApiError::new(StatusCode::BAD_REQUEST, code, e.message.clone());
    match e.code.as_str() {
        "invalid_action" => bad_request("invalid_action"),
        "stale_mark" => bad_request("stale_mark"),
        "ambiguous_text" => bad_request("ambiguous_text"),
        "text_not_found" => bad_request("text_not_found"),
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
    if !body.actions.is_empty() {
        control_lock::check(&state, device, &user).await?;
    }
    let req = |request_id| {
        server_message::Msg::Act(proto::ActRequest {
            request_id,
            actions: body.actions.into_iter().flat_map(Action::into_proto).collect(),
            observe: match body.observe {
                Observe::Shot => proto::ObserveMode::Shot,
                Observe::None => proto::ObserveMode::None,
            }
            .into(),
            display: match body.display {
                Some(DisplaySel::Index(i)) => Some(i),
                _ => None,
            },
            all_displays: matches!(body.display, Some(DisplaySel::All(_))),
            controller: user.id.to_string(),
            marks: body.marks,
        })
    };
    let agent_message::Msg::ActResult(result) = state.agents.request(device, req).await? else {
        return Err(anyhow::anyhow!("unexpected reply to ActRequest").into());
    };
    if let Some(e) = result.error {
        return Err(agent_error(e));
    }
    let images: Vec<Value> = result
        .images
        .into_iter()
        .map(|i| {
            let data = (!i.unchanged).then(|| base64::engine::general_purpose::STANDARD.encode(&i.data));
            json!({ "display": i.display, "width": i.width, "height": i.height, "unchanged": i.unchanged, "mime": i.mime, "data": data })
        })
        .collect();
    let mut reply = json!({ "images": images });
    if result.observation != 0 {
        reply["observation"] = result.observation.into();
        reply["marks"] = result
            .marks
            .iter()
            .map(|m| json!({ "number": m.number, "text": m.text, "role": m.role, "display": m.display, "x": m.x, "y": m.y, "width": m.width, "height": m.height }))
            .collect();
    }
    Ok(Json(reply))
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
