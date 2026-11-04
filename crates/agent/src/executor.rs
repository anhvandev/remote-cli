//! Executor: carries out the Actions of a request in order, then produces one Observation.

use std::collections::HashMap;

use rcli_protocol::agent::{action::Kind, ActRequest, ActResult, Error, Mark, ObserveMode, Point};

use crate::{
    coords, marks,
    observer::{self, Observer},
    platform::{Button, DisplayInfo, Input, Platform},
};

#[derive(Default)]
pub struct Executor {
    /// The display last acted on: the default display to observe (the primary one before any Action).
    last_display: Option<u32>,
    observer: Observer,
    /// Each Controller's latest Observation with Marks: (id, Mark table). Mark targets must come from it.
    // shortcut: like `Observer::last`, never evicted.
    marks: HashMap<String, (u64, Vec<Mark>)>,
    last_observation: u64,
}

/// Why a request failed: `invalid_action` (the Controller's mistake) or `platform` (the OS refused).
fn error(code: &str, e: impl std::fmt::Display) -> Error {
    Error { code: code.into(), message: e.to_string() }
}

impl Executor {
    pub fn act(&mut self, p: &dyn Platform, req: &ActRequest) -> ActResult {
        let mut result = self.run(p, req).unwrap_or_else(|e| ActResult { error: Some(e), ..Default::default() });
        result.request_id = req.request_id;
        result
    }

    fn run(&mut self, p: &dyn Platform, req: &ActRequest) -> Result<ActResult, Error> {
        let displays = p.displays().map_err(|e| error("platform", e))?;
        for action in &req.actions {
            let kind = action.kind.as_ref().ok_or_else(|| error("invalid_action", "empty action"))?;
            for input in self.inputs(p, &req.controller, &displays, kind)? {
                p.input(input).map_err(|e| error("platform", e))?;
            }
        }
        if req.observe() == ObserveMode::None {
            return Ok(ActResult::default());
        }
        let wanted: Vec<u32> = match (req.all_displays, req.display) {
            (true, _) => displays.iter().map(|d| d.index).collect(),
            (false, Some(d)) => vec![find(&displays, d)?.index],
            (false, None) => vec![self.last_display.or(displays.iter().find(|d| d.primary).map(|d| d.index)).unwrap_or(0)],
        };
        let (images, marks) = self.observer.shot(p, &wanted, &req.controller, req.marks).map_err(|e| error("platform", e))?;
        if !req.marks {
            return Ok(ActResult { images, ..Default::default() });
        }
        self.last_observation += 1;
        self.marks.insert(req.controller.clone(), (self.last_observation, marks.clone()));
        Ok(ActResult { images, observation: self.last_observation, marks, ..Default::default() })
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
                let (key, modifiers) = keys.split_last().unwrap();
                let mut inputs: Vec<Input> = modifiers.iter().map(|m| Input::KeyDown(m.to_string())).collect();
                inputs.push(Input::Key(key.to_string()));
                inputs.extend(modifiers.iter().rev().map(|m| Input::KeyUp(m.to_string())));
                inputs
            }
        })
    }

    /// Pointer move to a point or Mark target; remembers the display as last acted on.
    fn at(&mut self, p: &dyn Platform, controller: &str, displays: &[DisplayInfo], pt: &Point) -> Result<Input, Error> {
        let pt = &self.resolve(p, controller, displays, pt)?;
        let d = find(displays, pt.display)?;
        let (x, y) = coords::to_real(d, pt.x, pt.y).ok_or_else(|| {
            let (w, h) = coords::observed_size(d.width, d.height);
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

fn find(displays: &[DisplayInfo], index: u32) -> Result<&DisplayInfo, Error> {
    displays.iter().find(|d| d.index == index).ok_or_else(|| error("invalid_action", format!("no display {index}")))
}
