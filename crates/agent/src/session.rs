//! The daemon ↔ session helper split (macOS ticket 24, Linux ticket 16, Windows ticket 22; ADR 0001). A root/SYSTEM
//! service cannot reach the window server or input desktop of the logged-in person, so a helper process in the
//! console session connects back over a local stream and does the screen work: displays, capture, input, the
//! accessibility tree and the session state. Everything else (shell, power, metrics) stays in the daemon.
//!
//! Wire: one JSON request per line; one JSON reply per line (`{"ok": …}` or `{"error": …}`), and after a capture
//! reply its `len` raw RGBA bytes. `macos_session` (the Unix transport) and `windows_session` (named pipes) supply
//! the transport.

use std::{
    io::{BufRead, BufReader, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::platform::{
    DisplayInfo, Frame, Input, Inventory, Platform, PowerAction, Process, Pty, Resources, RunAs, SessionState, ShellOutput, UiElement,
};

/// What an input Action fails with at the macOS login window: CGEvent input never reaches it (ADR 0001). Linux's
/// greeter accepts XTest input, and Windows' SYSTEM helper can type at the sign-in screen and a UAC prompt.
pub const LOGIN_WINDOW_INPUT: &str = "input unavailable at macOS login window";

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    State,
    Displays,
    Capture {
        display: u32,
    },
    Input {
        input: Input,
    },
    Accessibility {
        display: u32,
    },
    /// Locking works differently in each session: let the helper that is in it do it.
    Lock,
}

/// One accepted helper connection. Calls are synchronous: a hung helper must not hang the Agent, so the transport
/// sets a read timeout and the daemon drops a helper that fails to answer.
pub struct Helper {
    reader: BufReader<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
}

impl Helper {
    pub fn new(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Self {
        Self { reader: BufReader::new(reader), writer }
    }

    /// One round trip: the `ok` value, plus the raw bytes that follow a capture reply.
    fn call(&mut self, request: &Request) -> anyhow::Result<(Value, Vec<u8>)> {
        self.writer.write_all(format!("{}\n", serde_json::to_string(request)?).as_bytes())?;
        let mut line = String::new();
        anyhow::ensure!(self.reader.read_line(&mut line)? > 0, "the session helper went away");
        let reply: Value = serde_json::from_str(&line)?;
        let mut bytes = vec![0; reply["len"].as_u64().unwrap_or(0) as usize];
        self.reader.read_exact(&mut bytes)?;
        match reply["error"].as_str() {
            Some(e) => anyhow::bail!("{e}"),
            None => Ok((reply["ok"].clone(), bytes)),
        }
    }
}

/// The daemon's Platform: screen calls go to the session helper on the console, the rest to `inner`.
pub struct SessionPlatform {
    inner: Arc<dyn Platform>,
    helpers: Arc<Mutex<Vec<Helper>>>,
    /// Whether input is allowed at the login screen. macOS CGEvents never reach the login window; Linux's greeter
    /// accepts XTest input, and Windows' SYSTEM helper can type there (and at a UAC prompt), so they must.
    allow_login_input: bool,
}

impl SessionPlatform {
    pub fn new(inner: Arc<dyn Platform>, allow_login_input: bool) -> Self {
        Self { inner, helpers: Default::default(), allow_login_input }
    }

    /// The accepted helpers, for the transport's accept loop to push into.
    pub fn helpers(&self) -> Arc<Mutex<Vec<Helper>>> {
        self.helpers.clone()
    }

    /// Forgets every connected helper: its session ended, or another session came to the console.
    pub fn clear_helpers(&self) {
        self.helpers.lock().unwrap().clear();
    }

    /// How many helpers are connected (a test and the Linux reconciler use it).
    pub fn helper_count(&self) -> usize {
        self.helpers.lock().unwrap().len()
    }

    /// Runs `f` on the helper whose session is on the console, with that session's state. Helpers that do not
    /// answer are dropped (their session ended).
    fn with_active<T>(&self, f: impl FnOnce(&mut Helper, SessionState) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let mut helpers = self.helpers.lock().unwrap();
        let mut active = None;
        let mut i = 0;
        while i < helpers.len() {
            match helpers[i].call(&Request::State) {
                Ok((v, _)) => {
                    let state = serde_json::from_value(v["state"].clone()).ok();
                    if let (None, Some(state), true) = (&active, state, v["on_console"] == true) {
                        active = Some((i, state));
                    }
                    i += 1;
                }
                Err(_) => {
                    helpers.remove(i);
                }
            }
        }
        let (i, state) = active.ok_or_else(|| anyhow::anyhow!("no GUI session: the rcli session agent is not running"))?;
        f(&mut helpers[i], state)
    }

    fn call(&self, request: Request) -> anyhow::Result<(Value, Vec<u8>)> {
        self.with_active(|helper, _| helper.call(&request))
    }
}

impl Platform for SessionPlatform {
    fn hostname(&self) -> String {
        self.inner.hostname()
    }
    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        let (v, _) = self.call(Request::Displays)?;
        Ok(v.as_array().map(Vec::as_slice).unwrap_or_default().iter().map(display_from).collect())
    }
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        let (v, rgba) = self.call(Request::Capture { display })?;
        let (width, height) = (v["width"].as_u64().unwrap_or(0) as u32, v["height"].as_u64().unwrap_or(0) as u32);
        anyhow::ensure!(rgba.len() == width as usize * height as usize * 4, "bad frame from the session helper");
        Ok(Frame { width, height, rgba })
    }
    fn input(&self, input: Input) -> anyhow::Result<()> {
        self.with_active(|helper, state| {
            if !self.allow_login_input {
                anyhow::ensure!(state != SessionState::LoginScreen, LOGIN_WINDOW_INPUT);
            }
            helper.call(&Request::Input { input }).map(drop)
        })
    }
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        Ok(serde_json::from_value(self.call(Request::Accessibility { display })?.0)?)
    }
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        self.inner.recognize_text(frame)
    }
    /// No helper connected means no GUI session yet: the login window (or a machine still booting).
    fn session_state(&self) -> anyhow::Result<SessionState> {
        Ok(self.with_active(|_, state| Ok(state)).unwrap_or(SessionState::LoginScreen))
    }
    fn inventory(&self) -> anyhow::Result<Inventory> {
        self.inner.inventory()
    }
    fn metrics(&self) -> anyhow::Result<Resources> {
        self.inner.metrics()
    }
    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        self.inner.processes()
    }
    fn shell(&self, command: &str, run_as: RunAs, timeout: Duration) -> anyhow::Result<ShellOutput> {
        self.inner.shell(command, run_as, timeout)
    }
    fn pty(&self, run_as: RunAs, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        self.inner.pty(run_as, cols, rows)
    }
    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        match action {
            PowerAction::Lock => self.call(Request::Lock).map(drop),
            _ => self.inner.power(action),
        }
    }
    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        self.inner.send_wake(mac)
    }
    fn install_update(&self, version: &str, build: &[u8]) -> anyhow::Result<()> {
        self.inner.install_update(version, build)
    }
    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()> {
        self.inner.set_control_indicator(on)
    }
    fn ask_root(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        self.inner.ask_root(requester, timeout)
    }
    fn ask_accept(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        self.inner.ask_accept(requester, timeout)
    }
}

/// The helper side: answers the daemon's requests from `platform` until the connection closes. `on_console` says
/// whether this helper's session is the one on the console (a Mac or a Linux box may have several logged-in
/// sessions).
pub fn answer(
    reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    platform: &dyn Platform,
    on_console: &dyn Fn() -> bool,
) -> anyhow::Result<()> {
    for line in BufReader::new(reader).lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let mut bytes = vec![];
        let ok = match request {
            Request::State => platform.session_state().map(|s| json!({ "state": s, "on_console": on_console() })),
            Request::Displays => platform.displays().map(|d| Value::Array(d.iter().map(display_json).collect())),
            Request::Capture { display } => platform.capture(display).map(|f| {
                bytes = f.rgba;
                json!({ "width": f.width, "height": f.height })
            }),
            Request::Input { input } => platform.input(input).map(|()| Value::Null),
            Request::Accessibility { display } => platform.accessibility(display).map(|a| json!(a)),
            Request::Lock => platform.power(PowerAction::Lock).map(|()| Value::Null),
        };
        let reply = match ok {
            Ok(ok) => json!({ "ok": ok, "len": bytes.len() }),
            Err(e) => json!({ "error": format!("{e:#}") }),
        };
        writer.write_all(format!("{reply}\n").as_bytes())?;
        writer.write_all(&bytes)?;
    }
    Ok(())
}

fn display_json(d: &DisplayInfo) -> Value {
    json!({ "index": d.index, "x": d.x, "y": d.y, "width": d.width, "height": d.height, "scale": d.scale, "primary": d.primary })
}

fn display_from(v: &Value) -> DisplayInfo {
    let n = |k: &str| v[k].as_i64().unwrap_or(0);
    DisplayInfo {
        index: n("index") as u32,
        x: n("x") as i32,
        y: n("y") as i32,
        width: n("width") as u32,
        height: n("height") as u32,
        scale: v["scale"].as_f64().unwrap_or(1.0),
        primary: v["primary"] == true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::FakePlatform;

    /// A daemon-side SessionPlatform with one helper serving `screen` over a pair of in-memory pipes.
    fn platform_with_helper(screen: FakePlatform, allow_login_input: bool) -> SessionPlatform {
        let platform = SessionPlatform::new(Arc::new(FakePlatform::new("host")), allow_login_input);
        // Requests go daemon → helper, replies helper → daemon.
        let (requests_reader, requests_writer) = std::io::pipe().expect("anonymous pipe");
        let (replies_reader, replies_writer) = std::io::pipe().expect("anonymous pipe");
        platform.helpers().lock().unwrap().push(Helper::new(Box::new(replies_reader), Box::new(requests_writer)));
        std::thread::spawn(move || {
            let _ = answer(Box::new(requests_reader), Box::new(replies_writer), &screen, &|| true);
        });
        platform
    }

    #[test]
    fn screen_calls_reach_the_session_on_the_console() {
        let screen = FakePlatform::new("session");
        screen.set_session_state(SessionState::Locked);
        let daemon = platform_with_helper(screen.clone(), false);
        assert_eq!(daemon.session_state().unwrap(), SessionState::Locked);
        assert_eq!(daemon.displays().unwrap(), screen.displays().unwrap());
        assert_eq!(daemon.capture(0).unwrap(), screen.capture(0).unwrap());
        daemon.input(Input::Key("a".into())).unwrap();
        assert_eq!(screen.inputs(), vec![Input::Key("a".into())], "the key reached the lock screen session");
        daemon.power(PowerAction::Lock).unwrap();
        assert_eq!(screen.power_actions(), vec![PowerAction::Lock], "the session locked itself");
    }

    #[test]
    fn at_the_login_window_capture_works_and_input_is_unavailable_where_the_platform_says_so() {
        let screen = FakePlatform::new("loginwindow");
        screen.set_session_state(SessionState::LoginScreen);
        let daemon = platform_with_helper(screen.clone(), false);
        assert_eq!(daemon.session_state().unwrap(), SessionState::LoginScreen);
        assert!(daemon.capture(0).is_ok());
        let err = daemon.input(Input::Key("a".into())).unwrap_err();
        assert_eq!(err.to_string(), "input unavailable at macOS login window");
        assert!(screen.inputs().is_empty());

        // Linux's greeter and Windows' SYSTEM helper may type at the login screen and a UAC prompt.
        let screen = FakePlatform::new("greeter");
        screen.set_session_state(SessionState::LoginScreen);
        let daemon = platform_with_helper(screen.clone(), true);
        daemon.input(Input::Text("alice".into())).unwrap();
        assert_eq!(screen.inputs(), vec![Input::Text("alice".into())]);
    }

    #[test]
    fn without_a_helper_it_reports_the_login_window_and_cannot_capture() {
        let daemon = SessionPlatform::new(Arc::new(FakePlatform::new("host")), false);
        assert_eq!(daemon.session_state().unwrap(), SessionState::LoginScreen);
        assert!(daemon.capture(0).unwrap_err().to_string().contains("no GUI session"));
    }
}
