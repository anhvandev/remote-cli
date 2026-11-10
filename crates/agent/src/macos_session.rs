//! The installed macOS Agent (ticket 24, ADR 0001) is a root LaunchDaemon, and a daemon cannot reach the window
//! server. A LaunchAgent in every GUI session (LoginWindow and Aqua) connects back to it over a local socket and
//! does the screen work: displays, capture, input, the accessibility tree and the session state. The daemon sends
//! those calls to the helper whose session is on the console and keeps everything else (shell, power, metrics).
//! Wire: one JSON request per line; one JSON reply per line (`{"ok": …}` or `{"error": …}`), and after a capture
//! reply its `len` raw RGBA bytes.

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::platform::{
    DisplayInfo, Frame, Input, Inventory, Platform, PowerAction, Process, Pty, Resources, RunAs, SessionState,
    ShellOutput, UiElement,
};

/// What an input Action fails with at the login window: CGEvent input never reaches it (ADR 0001).
pub const LOGIN_WINDOW_INPUT: &str = "input unavailable at macOS login window";

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    State,
    Displays,
    Capture { display: u32 },
    Input { input: Input },
    Accessibility { display: u32 },
    /// Locking works only from inside the user's session.
    Lock,
}

/// The daemon's Platform: screen calls go to the session helper on the console, the rest to `inner`.
pub struct SessionPlatform {
    inner: Arc<dyn Platform>,
    helpers: Arc<Mutex<Vec<Helper>>>,
}

struct Helper {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Helper {
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

impl SessionPlatform {
    /// Listens on `socket` for session helpers run by root (the login window) or by the console user.
    pub fn listen(inner: Arc<dyn Platform>, socket: &Path) -> anyhow::Result<Arc<Self>> {
        Self::listen_with(inner, socket, |uid| uid == 0 || console_uid() == Some(uid))
    }

    /// `listen` with the peer check `trust(uid)` given (tests run as an ordinary user).
    pub fn listen_with(
        inner: Arc<dyn Platform>,
        socket: &Path,
        trust: impl Fn(u32) -> bool + Send + 'static,
    ) -> anyhow::Result<Arc<Self>> {
        if let Some(dir) = socket.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let _ = std::fs::remove_file(socket);
        let listener = UnixListener::bind(socket)?;
        // The user's helper must be able to connect; the peer uid, not the file mode, decides who is kept.
        std::fs::set_permissions(socket, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
        let helpers: Arc<Mutex<Vec<Helper>>> = Default::default();
        let accepted = helpers.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !peer_uid(&stream).is_some_and(&trust) {
                    continue;
                }
                // A hung helper must not hang the Agent: it is dropped like one that went away.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let Ok(writer) = stream.try_clone() else { continue };
                accepted.lock().unwrap().push(Helper { reader: BufReader::new(stream), writer });
            }
        });
        Ok(Arc::new(Self { inner, helpers }))
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
            anyhow::ensure!(state != SessionState::LoginScreen, LOGIN_WINDOW_INPUT);
            helper.call(&Request::Input { input }).map(drop)
        })
    }
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        Ok(serde_json::from_value(self.call(Request::Accessibility { display })?.0)?)
    }
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        self.inner.recognize_text(frame)
    }
    /// No helper connected means no GUI session yet: the login window (or a Mac still booting).
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

/// The session helper (`rcli agent session`): connects to the daemon at `socket` and answers its screen calls from
/// `platform`, reconnecting whenever the daemon restarts. `on_console` says whether this session is the one shown.
pub fn run_helper(socket: &Path, platform: &dyn Platform, on_console: &dyn Fn() -> bool) -> ! {
    loop {
        if let Ok(stream) = UnixStream::connect(socket) {
            if let Err(e) = answer(stream, platform, on_console) {
                tracing::debug!("session helper connection ended: {e:#}");
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Answers the daemon's requests on one connection until it closes.
pub fn answer(stream: UnixStream, platform: &dyn Platform, on_console: &dyn Fn() -> bool) -> anyhow::Result<()> {
    let mut writer = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
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

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    (unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
}

/// The user logged in on the console: the owner of `/dev/console` (root at the login window).
fn console_uid() -> Option<u32> {
    std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/dev/console").ok()?).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::FakePlatform;

    /// A daemon-side SessionPlatform with one helper serving `screen` (a FakePlatform session) over a real socket.
    fn daemon_with_helper(screen: FakePlatform, on_console: bool) -> (Arc<SessionPlatform>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rcli-session-{}-{}", std::process::id(), rand_suffix()));
        let socket = dir.join("session.sock");
        let daemon = SessionPlatform::listen_with(Arc::new(FakePlatform::new("mac")), &socket, |_| true).unwrap();
        let path = socket.clone();
        std::thread::spawn(move || {
            let stream = UnixStream::connect(&path).unwrap();
            let _ = answer(stream, &screen, &move || on_console);
        });
        for _ in 0..200 {
            if !daemon.helpers.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (daemon, dir)
    }

    fn rand_suffix() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
    }

    #[test]
    fn screen_calls_reach_the_session_on_the_console() {
        let screen = FakePlatform::new("session");
        screen.set_session_state(SessionState::Locked);
        let (daemon, dir) = daemon_with_helper(screen.clone(), true);
        assert_eq!(daemon.session_state().unwrap(), SessionState::Locked);
        assert_eq!(daemon.displays().unwrap(), screen.displays().unwrap());
        assert_eq!(daemon.capture(0).unwrap(), screen.capture(0).unwrap());
        daemon.input(Input::Key("a".into())).unwrap();
        assert_eq!(screen.inputs(), vec![Input::Key("a".into())], "the key reached the lock screen session");
        daemon.power(PowerAction::Lock).unwrap();
        assert_eq!(screen.power_actions(), vec![PowerAction::Lock], "the session locked itself");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn at_the_login_window_capture_works_and_input_is_unavailable() {
        let screen = FakePlatform::new("loginwindow");
        screen.set_session_state(SessionState::LoginScreen);
        let (daemon, dir) = daemon_with_helper(screen.clone(), true);
        assert_eq!(daemon.session_state().unwrap(), SessionState::LoginScreen);
        assert!(daemon.capture(0).is_ok());
        let err = daemon.input(Input::Key("a".into())).unwrap_err();
        assert_eq!(err.to_string(), "input unavailable at macOS login window");
        assert!(screen.inputs().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_a_session_on_the_console_it_reports_the_login_window_and_cannot_capture() {
        let (daemon, dir) = daemon_with_helper(FakePlatform::new("switched-out user"), false);
        assert_eq!(daemon.session_state().unwrap(), SessionState::LoginScreen);
        assert!(daemon.capture(0).unwrap_err().to_string().contains("no GUI session"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
