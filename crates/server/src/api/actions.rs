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

use super::{agent_error, bad_request, devices::resolve_for, ApiError, Caller};
use crate::{agent_link, audit, auth::User, control_lock, metrics};

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/api/v1/devices/{id}/actions", post(act))
        .route("/api/v1/devices/{id}/displays", get(displays))
}

/// `{"actions": [...], "observe": "shot" | "none" | "frames" | "clip", "display": N | "all", "max_frames": N}`.
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
    /// Detect Marks: annotated images plus `observation` id and `marks` table in the reply.
    #[serde(default)]
    marks: bool,
    /// The caller's open media channel (`POST .../media`): large images and clips go over it peer-to-peer.
    #[serde(default)]
    media_session: String,
    /// Wait Condition: wait (after the Actions) until it holds, then observe.
    until: Option<Until>,
}

/// `{"appears" | "disappears" | "value" | "state": {"text": "..", "role": "..", ..}, "timeout_ms": N}`.
#[derive(Deserialize)]
struct Until {
    #[serde(flatten)]
    condition: Condition,
    timeout_ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Condition {
    Appears(WaitTarget),
    Disappears(WaitTarget),
    Value(ValueTarget),
    State(StateTarget),
}

/// The Marks a Wait Condition is about: text matched as for text targets, optionally narrowed by role.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitTarget {
    text: String,
    role: Option<String>,
}

/// `{"text", "role"?, "equals" | "contains"}`; the Agent refuses anything but exactly one of the two.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValueTarget {
    text: String,
    role: Option<String>,
    equals: Option<String>,
    contains: Option<String>,
}

/// `{"text", "role"?, "has" | "lacks": "<Mark State>"}`; the Agent refuses anything but exactly one of the two.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateTarget {
    text: String,
    role: Option<String>,
    has: Option<String>,
    lacks: Option<String>,
}

impl Until {
    fn timeout_ms(&self) -> u32 {
        self.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS)
    }
}

impl From<Until> for proto::WaitCondition {
    fn from(u: Until) -> Self {
        use proto::wait_condition::Condition as C;
        let timeout_ms = u.timeout_ms();
        let (text, role, condition) = match u.condition {
            Condition::Appears(t) => (t.text, t.role, C::Appears(true)),
            Condition::Disappears(t) => (t.text, t.role, C::Disappears(true)),
            Condition::Value(v) => (v.text, v.role, C::Value(proto::ValueCondition { equals: v.equals, contains: v.contains })),
            Condition::State(s) => (s.text, s.role, C::State(proto::StateCondition { has: s.has, lacks: s.lacks })),
        };
        Self { text, role, timeout_ms, condition: Some(condition) }
    }
}

/// Upper bound of `max_frames`.
const MAX_FRAMES: u32 = 30;
/// Upper bound of all waits in one request (fixed waits and Wait Condition timeouts); the Agent request timeout grows by them.
const MAX_TOTAL_WAIT_MS: u64 = 30_000;
const DEFAULT_WAIT_TIMEOUT_MS: u32 = 5_000;

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Observe {
    #[default]
    Shot,
    None,
    Frames,
    Clip,
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

/// A target on `display` (default 0): a point in Observation image space (real pixels with `native`, as Live View
/// direct control sends), a Mark number from Observation `observation`, or a Mark's visible text.
#[derive(Deserialize)]
struct Point {
    #[serde(default)]
    display: u32,
    #[serde(default)]
    native: bool,
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
        // `"secret": true` (a password field) is read only by `mask_secrets`.
    },
    /// Key combination, e.g. "ctrl+shift+t".
    Key {
        keys: String,
    },
    /// Action Batch pause, in milliseconds, then (if given) until a Wait Condition holds.
    Wait {
        #[serde(default)]
        ms: u32,
        until: Option<Until>,
    },
}

impl From<Point> for proto::Point {
    fn from(p: Point) -> Self {
        let display = p.display;
        match p.target {
            Target::Xy { x, y } => Self { display, x, y, native: p.native, ..Default::default() },
            Target::Mark { mark, observation } => Self { display, mark: Some(mark), observation, ..Default::default() },
            Target::Text { text } => Self { display, text: Some(text), ..Default::default() },
        }
    }
}

impl Action {
    /// The protocol Actions this one becomes (typing into a target is a click, then the text; a wait with a
    /// Wait Condition is the fixed wait, then the Wait Condition).
    fn into_proto(self) -> Vec<proto::Action> {
        let kind = match self {
            Action::Type { text, into: Some(at) } => {
                return [Kind::Click(at.into()), Kind::TypeText(text)].map(|k| proto::Action { kind: Some(k) }).to_vec()
            }
            Action::Wait { ms, until: Some(u) } => {
                return [Kind::WaitMs(ms), Kind::WaitUntil(u.into())].map(|k| proto::Action { kind: Some(k) }).to_vec()
            }
            Action::Move(p) => Kind::Move(p.into()),
            Action::Click(p) => Kind::Click(p.into()),
            Action::DoubleClick(p) => Kind::DoubleClick(p.into()),
            Action::RightClick(p) => Kind::RightClick(p.into()),
            Action::Drag { from, to } => Kind::Drag(proto::Drag { from: Some(from.into()), to: Some(to.into()) }),
            Action::Scroll { at, dx, dy } => Kind::Scroll(proto::Scroll { at: Some(at.into()), dx, dy }),
            Action::Type { text, into: None } => Kind::TypeText(text),
            Action::Key { keys } => Kind::Key(keys),
            Action::Wait { ms, until: None } => Kind::WaitMs(ms),
        };
        vec![proto::Action { kind: Some(kind) }]
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_action", message)
}

/// The first Agent version with Wait Conditions; older Agents ignore them and answer at once, as if they were met.
const WAIT_CONDITIONS_SINCE: &str = "0.4.0";

impl ActBody {
    fn has_wait_condition(&self) -> bool {
        self.until.is_some() || self.actions.iter().any(|a| matches!(a, Action::Wait { until: Some(_), .. }))
    }
}

/// `agent_too_old` unless the Device's reported Agent version is semver and at least `WAIT_CONDITIONS_SINCE`
/// (a missing or non-semver version may be an Agent that would ignore the wait).
async fn refuse_agents_without_wait_conditions(state: &super::State, device: uuid::Uuid) -> Result<(), ApiError> {
    let version: Option<String> = sqlx::query_scalar("SELECT agent_version FROM devices WHERE id = $1")
        .bind(device)
        .fetch_one(&state.db)
        .await
        .map_err(anyhow::Error::from)?;
    let supported = version
        .as_deref()
        .is_some_and(|v| semver::Version::parse(v).is_ok() && !crate::updates::below(Some(v), WAIT_CONDITIONS_SINCE));
    if !supported {
        let message = format!(
            "the Device's Agent is {}; Wait Conditions need Agent {WAIT_CONDITIONS_SINCE} or later",
            version.as_deref().unwrap_or("of unknown version")
        );
        return Err(ApiError::new(StatusCode::CONFLICT, "agent_too_old", message));
    }
    Ok(())
}

async fn act(
    Caller(user): Caller,
    State(state): State<super::State>,
    Path(id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let device = resolve_for(&user, &state, &id).await?;
    let Json(mut params) = body.map_err(|e| bad_request(e.body_text()))?;
    let body: ActBody = serde_json::from_value(params.clone())
        .map_err(|e| bad_request(e.to_string()))?;
    let until_ms = |u: &Option<Until>| u.as_ref().map_or(0, |u| u64::from(u.timeout_ms()));
    let steps: u64 = body.actions.iter().map(|a| if let Action::Wait { ms, until } = a { u64::from(*ms) + until_ms(until) } else { 0 }).sum();
    let waits = steps + until_ms(&body.until);
    if waits > MAX_TOTAL_WAIT_MS {
        return Err(invalid(format!("waits add up to {waits} ms; at most {MAX_TOTAL_WAIT_MS} ms per request")));
    }
    let max_frames = body.max_frames.unwrap_or(10);
    if !(1..=MAX_FRAMES).contains(&max_frames) {
        return Err(invalid(format!("max_frames must be 1 to {MAX_FRAMES}")));
    }
    if body.until.is_some() && body.observe != Observe::Shot {
        return Err(invalid("until needs observe \"shot\""));
    }
    if body.has_wait_condition() {
        refuse_agents_without_wait_conditions(&state, device).await?;
    }
    // Every request from the holder, observe-only included, restarts the idle timer.
    control_lock::touch(&state, device, &user).await?;
    if !body.actions.is_empty() {
        control_lock::check(&state, device, &user).await?;
    }
    mask_secrets(&state, device, &mut params);
    let result = forward(&state, device, &user, body, max_frames, std::time::Duration::from_millis(waits)).await;
    let actions = params["actions"].as_array().cloned().unwrap_or_default();
    if actions.is_empty() {
        let outcome = json!({ "error": result.as_ref().err().map(|e| e.code) });
        audit::record(&state, &user, device, "act", params, outcome).await?;
        return result;
    }
    // One entry per Action of a batch: done, the failing step, or skipped after it.
    let failed = result.as_ref().err().map(|e| (e.code, e.details.get("step").and_then(Value::as_u64)));
    for (step, action) in actions.into_iter().enumerate() {
        let step = step as u64;
        let outcome = match failed {
            Some((_, Some(s))) if step > s => json!({ "skipped": true }),
            Some((_, Some(s))) if step < s => json!({ "error": null }),
            Some((code, _)) => json!({ "error": code }),
            None => json!({ "error": null }),
        };
        let mut entry = params.clone();
        entry["actions"] = json!([action]);
        entry["step"] = step.into();
        audit::record(&state, &user, device, "act", entry, outcome).await?;
    }
    result
}

/// Text typed into a password field (`secret`) or while the Device sits at a lock/login screen is masked.
fn mask_secrets(state: &super::State, device: uuid::Uuid, params: &mut Value) {
    let session = metrics::current_resources(state, device).and_then(|r| r.session);
    let at_lock_screen = session.is_some_and(|s| s != proto::SessionState::Active as i32);
    for a in params["actions"].as_array_mut().into_iter().flatten() {
        if a["type"] == "type" && (at_lock_screen || a["secret"] == true) {
            a["text"] = audit::MASK.into();
        }
    }
}

async fn forward(
    state: &super::State,
    device: uuid::Uuid,
    user: &User,
    body: ActBody,
    max_frames: u32,
    waits: std::time::Duration,
) -> Result<Json<Value>, ApiError> {
    // One API Action can expand to several protocol Actions (type into); map failures back to the API step.
    let (mut step_of, mut actions) = (vec![], vec![]);
    for (step, action) in body.actions.into_iter().enumerate() {
        for a in action.into_proto() {
            step_of.push(step as u32);
            actions.push(a);
        }
    }
    let req = proto::ActRequest {
        request_id: 0,
        actions,
        observe: match body.observe {
            Observe::Shot => proto::ObserveMode::Shot,
            Observe::None => proto::ObserveMode::None,
            Observe::Frames => proto::ObserveMode::Frames,
            Observe::Clip => proto::ObserveMode::Clip,
        }
        .into(),
        display: match body.display {
            Some(DisplaySel::Index(i)) => Some(i),
            _ => None,
        },
        all_displays: matches!(body.display, Some(DisplaySel::All(_))),
        controller: user.controller_display(),
        max_frames,
        marks: body.marks,
        media_session: body.media_session,
        until: body.until.map(Into::into),
    };
    send(state, device, req, &step_of, waits).await
}

/// Sends `req` (its request_id is filled in) and returns the Observation JSON.
/// `step_of[i]` = the API step of protocol Action `i`, reported as `step` on failure. `waits` lengthens the timeout.
async fn send(
    state: &super::State,
    device: uuid::Uuid,
    req: proto::ActRequest,
    step_of: &[u32],
    waits: std::time::Duration,
) -> Result<Json<Value>, ApiError> {
    let req = |request_id| server_message::Msg::Act(proto::ActRequest { request_id, ..req });
    let timeout = agent_link::REQUEST_TIMEOUT + agent_link::PROMPT_BUDGET + waits;
    let agent_message::Msg::ActResult(mut result) = state.agents.request_within(device, timeout, req).await? else {
        return Err(anyhow::anyhow!("unexpected reply to ActRequest").into());
    };
    if let Some(e) = result.error.take() {
        let mut err = agent_error(e);
        if let Some(step) = result.failed_step.and_then(|s| step_of.get(s as usize)) {
            err = err.with("step", *step);
        }
        // A timed-out Wait Condition still shows the screen as it was.
        if err.code == "wait_timeout" {
            err.details.extend(observation(result).as_object().cloned().unwrap_or_default());
        }
        return Err(err);
    }
    Ok(Json(observation(result)))
}

/// The Observation JSON of an Agent's ActResult.
fn observation(result: proto::ActResult) -> Value {
    let b64 = |data: &[u8]| base64::engine::general_purpose::STANDARD.encode(data);
    // Relayed, never stored (spec: no media in the audit log).
    // Bytes sent peer-to-peer are only referenced (`p2p`); the client puts them back into `data`.
    let p2p = |v: &mut Value, id: u64| {
        if id != 0 {
            v["data"] = Value::Null;
            v["p2p"] = id.into();
        }
    };
    let clip = result.clip.map(|c| {
        let mut v = json!({ "display": c.display, "width": c.width, "height": c.height, "mime": c.mime, "duration_ms": c.duration_ms, "data": b64(&c.data) });
        p2p(&mut v, c.p2p);
        v
    });
    let images: Vec<Value> = result
        .images
        .into_iter()
        .map(|i| {
            let data = (!i.unchanged).then(|| b64(&i.data));
            let mut v = json!({ "display": i.display, "width": i.width, "height": i.height, "unchanged": i.unchanged, "mime": i.mime, "data": data, "at_ms": i.at_ms });
            p2p(&mut v, i.p2p);
            v
        })
        .collect();
    let mut reply = json!({ "images": images });
    if result.observation != 0 {
        reply["observation"] = result.observation.into();
        reply["marks"] = result
            .marks
            .iter()
            .map(|m| {
                let mut v = json!({ "number": m.number, "text": m.text, "role": m.role, "display": m.display, "x": m.x, "y": m.y, "width": m.width, "height": m.height });
                if !m.states.is_empty() {
                    v["states"] = json!(m.states);
                }
                if !m.value.is_empty() {
                    v["value"] = json!(m.value);
                }
                if let Some(parent) = m.parent {
                    v["parent"] = json!(parent);
                }
                v
            })
            .collect();
    }
    if let Some(clip) = clip {
        reply["clip"] = clip;
    }
    reply
}

async fn displays(Caller(user): Caller, State(state): State<super::State>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let device = resolve_for(&user, &state, &id).await?;
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
