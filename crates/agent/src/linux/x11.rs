//! X11 screen and input: RandR monitors and GetImage capture over x11rb, input over XTest through enigo.

use enigo::{Axis, Coordinate, Direction, Key, Keyboard, Mouse};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::ConnectionExt as _,
        xproto::{ConnectionExt as _, ImageFormat, Window},
    },
    rust_connection::RustConnection,
};

use crate::platform::{Button, DisplayInfo, Frame, Input};

pub struct X {
    conn: RustConnection,
    root: Window,
}

impl X {
    pub fn connect() -> anyhow::Result<Self> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self { conn, root })
    }

    /// RandR monitors with their names. Exactly one is primary: the first one when none is marked.
    pub fn monitors(&self) -> anyhow::Result<Vec<(String, DisplayInfo)>> {
        let mut out = vec![];
        for (i, m) in self.conn.randr_get_monitors(self.root, true)?.reply()?.monitors.into_iter().enumerate() {
            let name = String::from_utf8_lossy(&self.conn.get_atom_name(m.name)?.reply()?.name).into_owned();
            let (x, y, width, height) = (m.x.into(), m.y.into(), m.width.into(), m.height.into());
            out.push((name, DisplayInfo { index: i as u32, x, y, width, height, scale: 1.0, primary: m.primary }));
        }
        if out.is_empty() {
            let g = self.conn.get_geometry(self.root)?.reply()?;
            out.push(("screen".into(), DisplayInfo { index: 0, x: 0, y: 0, width: g.width.into(), height: g.height.into(), scale: 1.0, primary: true }));
        }
        if !out.iter().any(|(_, d)| d.primary) {
            out[0].1.primary = true;
        }
        Ok(out)
    }

    // shortcut: plain GetImage (one copy over the socket); move to MIT-SHM if live streaming needs more frames per second.
    pub fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        let d = self.monitors()?.into_iter().map(|(_, d)| d).find(|d| d.index == display);
        let d = d.ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
        let (w, h) = (d.width as u16, d.height as u16);
        let image = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, d.x as i16, d.y as i16, w, h, !0)?.reply()?;
        // 24/32-bit TrueColor is BGRX in memory on every little-endian X server.
        anyhow::ensure!(image.data.len() == d.width as usize * d.height as usize * 4, "unsupported X visual (depth {})", image.depth);
        let rgba = image.data.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        Ok(Frame { width: d.width, height: d.height, rgba })
    }
}

pub fn input(enigo: &mut enigo::Enigo, displays: &[DisplayInfo], input: Input) -> anyhow::Result<()> {
    let direction = |down| if down { Direction::Press } else { Direction::Release };
    match input {
        Input::Move { display, x, y } => {
            let d = displays.iter().find(|d| d.index == display).ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
            enigo.move_mouse(d.x + x as i32, d.y + y as i32, Coordinate::Abs)?;
        }
        Input::Press { button, down } => {
            let button = match button {
                Button::Left => enigo::Button::Left,
                Button::Right => enigo::Button::Right,
                Button::Middle => enigo::Button::Middle,
            };
            enigo.button(button, direction(down))?;
        }
        Input::Scroll { dx, dy } => {
            if dy != 0 {
                enigo.scroll(dy, Axis::Vertical)?;
            }
            if dx != 0 {
                enigo.scroll(dx, Axis::Horizontal)?;
            }
        }
        Input::Text(text) => enigo.text(&text)?,
        Input::Key(name) => enigo.key(key(&name)?, Direction::Click)?,
        Input::KeyDown(name) => enigo.key(key(&name)?, Direction::Press)?,
        Input::KeyUp(name) => enigo.key(key(&name)?, Direction::Release)?,
    }
    Ok(())
}

/// A key name of a key combination (`"ctrl"`, `"enter"`, `"f5"`, `"a"`).
fn key(name: &str) -> anyhow::Result<Key> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(Key::Unicode(c));
    }
    let lower = name.to_lowercase();
    Ok(match lower.as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" => Key::Insert,
        "space" => Key::Space,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "capslock" => Key::CapsLock,
        "ctrl" | "control" => Key::Control,
        "shift" => Key::Shift,
        "alt" => Key::Alt,
        "super" | "meta" | "win" | "cmd" => Key::Meta,
        f if f.starts_with('f') => match f[1..].parse() {
            Ok(1) => Key::F1,
            Ok(2) => Key::F2,
            Ok(3) => Key::F3,
            Ok(4) => Key::F4,
            Ok(5) => Key::F5,
            Ok(6) => Key::F6,
            Ok(7) => Key::F7,
            Ok(8) => Key::F8,
            Ok(9) => Key::F9,
            Ok(10) => Key::F10,
            Ok(11) => Key::F11,
            Ok(12) => Key::F12,
            _ => anyhow::bail!("unknown key {name:?}"),
        },
        _ => anyhow::bail!("unknown key {name:?}"),
    })
}
