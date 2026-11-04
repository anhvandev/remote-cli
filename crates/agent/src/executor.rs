//! Executor: carries out the Actions of a request in order, then produces one Observation.

use rcli_protocol::agent::{action::Kind, ActRequest, ActResult, Error, ObserveMode, Point};

use crate::{
    coords,
    observer::Observer,
    platform::{Button, DisplayInfo, Input, Platform},
};

#[derive(Default)]
pub struct Executor {
    /// The display last acted on: the default display to observe (the primary one before any Action).
    last_display: Option<u32>,
    observer: Observer,
}

/// Why a request failed: `invalid_action` (the Controller's mistake) or `platform` (the OS refused).
fn error(code: &str, e: impl std::fmt::Display) -> Error {
    Error { code: code.into(), message: e.to_string() }
}

impl Executor {
    pub fn act(&mut self, p: &dyn Platform, req: &ActRequest) -> ActResult {
        let (images, error) = match self.run(p, req) {
            Ok(images) => (images, None),
            Err(e) => (vec![], Some(e)),
        };
        ActResult { request_id: req.request_id, images, error }
    }

    fn run(&mut self, p: &dyn Platform, req: &ActRequest) -> Result<Vec<rcli_protocol::agent::Image>, Error> {
        let displays = p.displays().map_err(|e| error("platform", e))?;
        for action in &req.actions {
            let kind = action.kind.as_ref().ok_or_else(|| error("invalid_action", "empty action"))?;
            for input in self.inputs(&displays, kind)? {
                p.input(input).map_err(|e| error("platform", e))?;
            }
        }
        if req.observe() == ObserveMode::None {
            return Ok(vec![]);
        }
        let wanted: Vec<u32> = match (req.all_displays, req.display) {
            (true, _) => displays.iter().map(|d| d.index).collect(),
            (false, Some(d)) => vec![find(&displays, d)?.index],
            (false, None) => vec![self.last_display.or(displays.iter().find(|d| d.primary).map(|d| d.index)).unwrap_or(0)],
        };
        self.observer.shot(p, &wanted, &req.controller).map_err(|e| error("platform", e))
    }

    /// The input primitives for one Action.
    fn inputs(&mut self, displays: &[DisplayInfo], kind: &Kind) -> Result<Vec<Input>, Error> {
        let press = |button, down| Input::Press { button, down };
        let click = |b| [press(b, true), press(b, false)];
        Ok(match kind {
            Kind::Move(pt) => vec![self.at(displays, pt)?],
            Kind::Click(pt) => [vec![self.at(displays, pt)?], click(Button::Left).to_vec()].concat(),
            Kind::DoubleClick(pt) => [vec![self.at(displays, pt)?], click(Button::Left).to_vec(), click(Button::Left).to_vec()].concat(),
            Kind::RightClick(pt) => [vec![self.at(displays, pt)?], click(Button::Right).to_vec()].concat(),
            Kind::Drag(d) => {
                let missing = || error("invalid_action", "drag needs from and to");
                let from = self.at(displays, d.from.as_ref().ok_or_else(missing)?)?;
                let to = self.at(displays, d.to.as_ref().ok_or_else(missing)?)?;
                vec![from, press(Button::Left, true), to, press(Button::Left, false)]
            }
            Kind::Scroll(s) => {
                let at = self.at(displays, s.at.as_ref().ok_or_else(|| error("invalid_action", "scroll needs a point"))?)?;
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

    /// Pointer move to an image-space point; remembers the display as last acted on.
    fn at(&mut self, displays: &[DisplayInfo], pt: &Point) -> Result<Input, Error> {
        let d = find(displays, pt.display)?;
        let (x, y) = coords::to_real(d, pt.x, pt.y).ok_or_else(|| {
            let (w, h) = coords::observed_size(d.width, d.height);
            error("invalid_action", format!("({}, {}) is outside display {} ({w}x{h})", pt.x, pt.y, pt.display))
        })?;
        self.last_display = Some(d.index);
        Ok(Input::Move { display: d.index, x, y })
    }
}

fn find(displays: &[DisplayInfo], index: u32) -> Result<&DisplayInfo, Error> {
    displays.iter().find(|d| d.index == index).ok_or_else(|| error("invalid_action", format!("no display {index}")))
}
