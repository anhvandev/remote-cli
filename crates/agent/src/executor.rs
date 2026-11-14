//! Executor: carries out the Actions of a request in order, then produces one Observation.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use rcli_protocol::agent::{
    action::Kind, wait_condition::Condition, Action, ActRequest, ActResult, Clip, Error, Mark, ObserveMode, Point, WaitCondition,
};

use crate::{
    clip, coords, marks,
    observer::{self, Frames, Observer, PerController},
    platform::{Button, DisplayInfo, Input, Platform},
};

#[derive(Default)]
pub struct Executor {
    /// The display last acted on: the default display to observe (the primary one before any Action).
    last_display: Option<u32>,
    observer: Observer,
    /// Each Controller's latest Observation with Marks: (id, Mark table). Mark targets must come from it.
    marks: PerController<(u64, Vec<Mark>)>,
    last_observation: u64,
}

/// Why a request failed: `invalid_action` (the Controller's mistake) or `platform` (the OS refused).
fn error(code: &str, e: impl std::fmt::Display) -> Error {
    Error { code: code.into(), message: e.to_string() }
}

impl Executor {
    pub fn act(&mut self, p: &dyn Platform, req: &ActRequest) -> ActResult {
        let mut failed_step = None;
        let mut result = self.run(p, req, &mut failed_step).unwrap_or_else(|e| ActResult { error: Some(e), ..Default::default() });
        result.request_id = req.request_id;
        result.failed_step = failed_step;
        result
    }

    fn run(&mut self, p: &dyn Platform, req: &ActRequest, failed_step: &mut Option<u32>) -> Result<ActResult, Error> {
        let displays = p.displays().map_err(|e| error("platform", e))?;
        if displays.is_empty() {
            return Err(error("no_display", "Device has no display"));
        }
        if req.observe() == ObserveMode::Clip {
            let clip = self.record(p, &displays, req, failed_step)?;
            return Ok(ActResult { clip: Some(clip), ..Default::default() });
        }
        // `frames` watches every display; the observed ones are picked once the Actions are done.
        let mut frames = (req.observe() == ObserveMode::Frames)
            .then(|| Frames::new(displays.iter().map(|d| d.index).collect(), req.max_frames));
        if let Some(f) = frames.as_mut() {
            f.record(p, Duration::ZERO).map_err(|e| error("platform", e))?;
        }
        // A timed-out wait (a batch step or `until`) still returns the Observation, with the error.
        let mut timeout = match self.run_actions(p, &displays, req, frames.as_mut(), failed_step) {
            Err(e) if e.code == "wait_timeout" => Some(e),
            ran => ran.map(|()| None)?,
        };
        if req.observe() == ObserveMode::None && timeout.is_none() {
            return Ok(ActResult::default());
        }
        let wanted = self.observed(&displays, req)?;
        if let Some(f) = frames {
            let images = self.observer.frames(p, f, &wanted, &req.controller).map_err(|e| error("platform", e))?;
            return Ok(ActResult { images, error: timeout, ..Default::default() });
        }
        if let (None, Some(u)) = (&timeout, &req.until) {
            timeout = wait_until(p, &wanted, u)?;
        }
        let (images, marks) = self.observer.shot(p, &wanted, &req.controller, req.marks).map_err(|e| error("platform", e))?;
        if !req.marks {
            return Ok(ActResult { images, error: timeout, ..Default::default() });
        }
        self.last_observation += 1;
        *self.marks.entry(&req.controller) = (self.last_observation, marks.clone());
        Ok(ActResult { images, observation: self.last_observation, marks, error: timeout, ..Default::default() })
    }

    /// The displays to observe: all, the requested one, else the one last acted on, else the primary one.
    fn observed(&self, displays: &[DisplayInfo], req: &ActRequest) -> Result<Vec<u32>, Error> {
        Ok(match (req.all_displays, req.display) {
            (true, _) => displays.iter().map(|d| d.index).collect(),
            (false, Some(d)) => vec![find(displays, d)?.index],
            (false, None) => vec![self.last_display.or(displays.iter().find(|d| d.primary).map(|d| d.index)).unwrap_or(0)],
        })
    }

    /// One Action of a batch: its inputs, then its wait (recording frames meanwhile if asked).
    fn step(
        &mut self,
        p: &dyn Platform,
        req: &ActRequest,
        displays: &[DisplayInfo],
        frames: Option<&mut Frames>,
        action: &Action,
    ) -> Result<(), Error> {
        let kind = action.kind.as_ref().ok_or_else(|| error("invalid_action", "empty action"))?;
        for input in self.inputs(p, &req.controller, displays, kind)? {
            p.input(input).map_err(|e| error("platform", e))?;
        }
        // A Wait Condition step watches the displays an Observation would show.
        if let Kind::WaitUntil(u) = kind {
            if let Some(timeout) = wait_until(p, &self.observed(displays, req)?, u)? {
                return Err(timeout);
            }
        }
        let wait = match kind {
            Kind::WaitMs(ms) => Duration::from_millis((*ms).into()),
            _ => Duration::ZERO,
        };
        match frames {
            Some(f) => f.record(p, wait).map_err(|e| error("platform", e)),
            None => {
                std::thread::sleep(wait);
                Ok(())
            }
        }
    }

    /// The Actions in order; on failure `failed_step` is the index of the failing one.
    fn run_actions(
        &mut self,
        p: &dyn Platform,
        displays: &[DisplayInfo],
        req: &ActRequest,
        mut frames: Option<&mut Frames>,
        failed_step: &mut Option<u32>,
    ) -> Result<(), Error> {
        for (i, action) in req.actions.iter().enumerate() {
            if let Err(e) = self.step(p, req, displays, frames.as_deref_mut(), action) {
                *failed_step = Some(i as u32);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Runs the Actions while recording one display: the requested one, else the first one acted on, else the default.
    fn record(
        &mut self,
        p: &dyn Platform,
        displays: &[DisplayInfo],
        req: &ActRequest,
        failed_step: &mut Option<u32>,
    ) -> Result<Clip, Error> {
        if req.all_displays {
            return Err(error("invalid_action", "clip records one display"));
        }
        let first_acted = req.actions.iter().find_map(|a| match a.kind.as_ref()? {
            Kind::Move(pt) | Kind::Click(pt) | Kind::DoubleClick(pt) | Kind::RightClick(pt) => Some(pt.display),
            Kind::Drag(d) => d.from.as_ref().map(|pt| pt.display),
            Kind::Scroll(s) => s.at.as_ref().map(|pt| pt.display),
            Kind::TypeText(_) | Kind::Key(_) | Kind::WaitMs(_) | Kind::WaitUntil(_) => None,
        });
        let display = match first_acted.filter(|_| req.display.is_none()) {
            Some(d) => find(displays, d)?.index,
            None => self.observed(displays, req)?[0],
        };
        let done = AtomicBool::new(false);
        std::thread::scope(|s| {
            let recorder = s.spawn(|| clip::record(p, display, &done));
            let ran = self.run_actions(p, displays, req, None, failed_step);
            done.store(true, Ordering::SeqCst);
            let clip = recorder.join().map_err(|_| error("platform", "clip recorder panicked"))?;
            ran?;
            clip.map_err(|e| error("platform", e))
        })
    }

    /// The input primitives for one Action.
    fn inputs(&mut self, p: &dyn Platform, controller: &str, displays: &[DisplayInfo], kind: &Kind) -> Result<Vec<Input>, Error> {
        let mut at = |pt: &Point| self.at(p, controller, displays, pt);
        let press = |button, down| Input::Press { button, down };
        let click = |b| [press(b, true), press(b, false)];
        Ok(match kind {
            Kind::Move(pt) => vec![at(pt)?],
            Kind::Click(pt) => [vec![at(pt)?], click(Button::Left).to_vec()].concat(),
            Kind::DoubleClick(pt) => [vec![at(pt)?], click(Button::Left).to_vec(), click(Button::Left).to_vec()].concat(),
            Kind::RightClick(pt) => [vec![at(pt)?], click(Button::Right).to_vec()].concat(),
            Kind::Drag(d) => {
                let missing = || error("invalid_action", "drag needs from and to");
                let from = at(d.from.as_ref().ok_or_else(missing)?)?;
                let to = at(d.to.as_ref().ok_or_else(missing)?)?;
                vec![from, press(Button::Left, true), to, press(Button::Left, false)]
            }
            Kind::Scroll(s) => {
                let at = at(s.at.as_ref().ok_or_else(|| error("invalid_action", "scroll needs a point"))?)?;
                vec![at, Input::Scroll { dx: s.dx, dy: s.dy }]
            }
            Kind::TypeText(text) => vec![Input::Text(text.clone())],
            Kind::Key(combo) => {
                let keys: Vec<&str> = combo.split('+').map(str::trim).collect();
                if keys.iter().any(|k| k.is_empty()) {
                    return Err(error("invalid_action", format!("bad key combination {combo:?}")));
                }
                // Ctrl+Alt+Del is the secure attention sequence, not three keys: Windows must call SendSAS.
                if is_sas(&keys) {
                    return Ok(vec![Input::Sas]);
                }
                let (key, modifiers) = keys.split_last().unwrap();
                let mut inputs: Vec<Input> = modifiers.iter().map(|m| Input::KeyDown(m.to_string())).collect();
                inputs.push(Input::Key(key.to_string()));
                inputs.extend(modifiers.iter().rev().map(|m| Input::KeyUp(m.to_string())));
                inputs
            }
            Kind::WaitMs(_) | Kind::WaitUntil(_) => vec![],
        })
    }

    /// Pointer move to a point or Mark target; remembers the display as last acted on.
    fn at(&mut self, p: &dyn Platform, controller: &str, displays: &[DisplayInfo], pt: &Point) -> Result<Input, Error> {
        let pt = &self.resolve(p, controller, displays, pt)?;
        let d = find(displays, pt.display)?;
        // Native points (Live View direct control) are real pixels already.
        let real = match pt.native {
            true => (pt.x < d.width && pt.y < d.height).then_some((pt.x, pt.y)),
            false => coords::to_real(d, pt.x, pt.y),
        };
        let (x, y) = real.ok_or_else(|| {
            let (w, h) = if pt.native { (d.width, d.height) } else { coords::observed_size(d.width, d.height) };
            error("invalid_action", format!("({}, {}) is outside display {} ({w}x{h})", pt.x, pt.y, pt.display))
        })?;
        self.last_display = Some(d.index);
        Ok(Input::Move { display: d.index, x, y })
    }
}

impl Executor {
    /// A `mark` or `text` target as an image-space point (the Mark's centre); plain points pass through.
    fn resolve(&self, p: &dyn Platform, controller: &str, displays: &[DisplayInfo], pt: &Point) -> Result<Point, Error> {
        let mark = if let Some(n) = pt.mark {
            let Some((latest, marks)) = self.marks.get(controller) else {
                return Err(error("stale_mark", "no Observation with Marks yet; observe with Marks first"));
            };
            if *latest != pt.observation {
                return Err(error(
                    "stale_mark",
                    format!("Mark {n} is from Observation {}, the latest with Marks is {latest}; observe again", pt.observation),
                ));
            }
            marks.iter().find(|m| m.number == n).cloned().ok_or_else(|| error("invalid_action", format!("Observation {latest} has no Mark {n}")))?
        } else if let Some(text) = &pt.text {
            let d = find(displays, pt.display)?;
            let frame = observer::settle(p, d.index).map_err(|e| error("platform", e))?;
            let marks = marks::detect(p, d.index, &frame, 1).map_err(|e| error("platform", e))?;
            marks::by_text(&marks, text)?.clone()
        } else {
            return Ok(pt.clone());
        };
        let (x, y) = marks::center(&mark);
        Ok(Point { display: mark.display, x, y, ..Default::default() })
    }
}

/// Re-detects Marks on `displays` (each reading settles the screen, so about every 100 ms) until two readings in
/// a row meet `until`; the `wait_timeout` error when its timeout passed first (never before two readings).
fn wait_until(p: &dyn Platform, displays: &[u32], until: &WaitCondition) -> Result<Option<Error>, Error> {
    let condition = until.condition.as_ref().ok_or_else(|| error("invalid_action", "Wait Condition without a condition"))?;
    let on_mark = match condition {
        Condition::Value(v) if v.equals.is_some() == v.contains.is_some() => {
            return Err(error("invalid_action", "a value Wait Condition needs exactly one of equals or contains"))
        }
        Condition::State(s) if s.has.is_some() == s.lacks.is_some() => {
            return Err(error("invalid_action", "a state Wait Condition needs exactly one of has or lacks"))
        }
        Condition::Value(_) | Condition::State(_) => true,
        Condition::Appears(_) | Condition::Disappears(_) => false,
    };
    // OCR Marks never carry values or Mark States, so such a wait could never be met.
    for &d in displays.iter().filter(|_| on_mark) {
        if p.accessibility(d).map_err(|e| error("platform", e))?.is_none() {
            return Err(error("no_accessibility_tree", "the screen has no accessibility tree, so no values or Mark States"));
        }
    }
    let deadline = Instant::now() + Duration::from_millis(until.timeout_ms.into());
    // Whether the previous reading met it; `None` before the first, so a timeout never comes before two readings.
    let mut met_before = None;
    loop {
        let mut all = vec![];
        for &d in displays {
            let frame = observer::settle(p, d).map_err(|e| error("platform", e))?;
            all.extend(marks::detect(p, d, &frame, 1).map_err(|e| error("platform", e))?);
        }
        let holds = condition_holds(condition, &all, &until.text, until.role.as_deref())?;
        if holds && met_before == Some(true) {
            return Ok(None);
        }
        if met_before.is_some() && Instant::now() >= deadline {
            let message = format!("Wait Condition on {:?} not met within {} ms", until.text, until.timeout_ms);
            return Ok(Some(error("wait_timeout", message)));
        }
        met_before = Some(holds);
    }
}

/// Whether one reading's Marks meet `condition` about the Marks matching `text` (and `role`). A value or Mark State
/// is read from the one Mark matching as a text target would; `ambiguous_text` when several do.
fn condition_holds(condition: &Condition, all: &[Mark], text: &str, role: Option<&str>) -> Result<bool, Error> {
    let target_mark = || match marks::by_text(&marks::with_role(all, role), text) {
        Ok(m) => Ok(Some(m.clone())),
        Err(e) if e.code == "text_not_found" => Ok(None),
        Err(e) => Err(e),
    };
    Ok(match condition {
        Condition::Appears(_) => marks::present(all, text, role),
        Condition::Disappears(_) => !marks::present(all, text, role),
        Condition::Value(v) => target_mark()?.is_some_and(|m| match (&v.equals, &v.contains) {
            (Some(want), _) => m.value == *want,
            (_, part) => m.value.contains(part.as_deref().unwrap_or_default()),
        }),
        // A Mark not on screen neither has nor lacks a state.
        Condition::State(s) => target_mark()?.is_some_and(|m| match (&s.has, &s.lacks) {
            (Some(state), _) => m.states.contains(state),
            (_, state) => !m.states.iter().any(|s| Some(s) == state.as_ref()),
        }),
    })
}

fn find(displays: &[DisplayInfo], index: u32) -> Result<&DisplayInfo, Error> {
    displays.iter().find(|d| d.index == index).ok_or_else(|| error("invalid_action", format!("no display {index}")))
}

/// Whether the key combination is Ctrl+Alt+Del, in any order and spelling (`ctrl+alt+del`, `alt+control+delete`).
fn is_sas(keys: &[&str]) -> bool {
    let mut has = (false, false, false);
    for key in keys {
        match key.to_lowercase().as_str() {
            "ctrl" | "control" => has.0 = true,
            "alt" | "option" => has.1 = true,
            "del" | "delete" => has.2 = true,
            _ => return false,
        }
    }
    has == (true, true, true) && keys.len() == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_alt_del_is_the_secure_attention_sequence_not_three_keys() {
        assert!(is_sas(&["ctrl", "alt", "del"]));
        assert!(is_sas(&["alt", "control", "delete"]));
        assert!(is_sas(&["Ctrl", "Alt", "Delete"]));
        assert!(!is_sas(&["ctrl", "alt"]), "two keys are an ordinary combination");
        assert!(!is_sas(&["ctrl", "alt", "shift"]));
        assert!(!is_sas(&["ctrl", "alt", "del", "shift"]));
        assert!(!is_sas(&["ctrl", "del"]));
    }
}
