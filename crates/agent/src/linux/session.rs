//! The Linux console session (ticket 16, Wayland added by ticket 19): the root service must show and control the
//! lock screen and the display-manager (greeter) login. It cannot use its own environment: on X11 the server's
//! cookie lives in the session's Xauthority file and enigo (input) reads `DISPLAY`/`XAUTHORITY` from the
//! environment; on Wayland the desktop portal that captures the screen lives on the user's session bus. So the
//! Agent service discovers seat0's active session with logind and runs `rcli agent session` as that session's user with
//! the right variables set. The helper answers the same wire protocol as the macOS session helper; displays,
//! capture, accessibility and session state go to it, while shell, power, metrics and inventory stay in the
//! Agent service.
//!
//! Input differs by session type: X11 goes through the helper (XTest reaches the lock screen and the greeter,
//! unlike CGEvent at the macOS login window), Wayland through a virtual device on `/dev/uinput` that only root can
//! create, so the Agent service writes it and the helper only captures.
//!
//! Why not talk to the X server directly? The root Agent service can read the cookie, but enigo would still need the
//! process-global environment, and that changes when the console session changes. The helper already existed for
//! macOS; here it doubles as the only place the session environment is set.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::{
    screencast::{TokenStore, TOKEN_ENV},
    uinput::VirtualInput,
    SessionKind,
};
use crate::{
    platform::{Frame, Input, Inventory, Platform, PowerAction, Process, Pty, Resources, Account, SessionState, ShellOutput, UiElement},
    macos_session::SessionPlatform,
};

/// Users a display manager runs its greeter as; their X server is the login screen, not a logged-in user.
const GREETERS: [&str; 7] = ["gdm", "gdm3", "lightdm", "sddm", "lxdm", "xdm", "kdm"];

/// One graphical session a helper can serve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub session: SessionKind,
    /// `DISPLAY` on X11 (`:0`), `WAYLAND_DISPLAY` on Wayland (`wayland-0`).
    pub display: String,
    pub user: String,
    pub uid: Option<u32>,
    /// The X server's authorization file, e.g. `/run/lightdm/root/:0`; a Wayland session has none.
    pub xauthority: Option<PathBuf>,
    /// Nobody is logged in: this is the display-manager greeter.
    pub greeter: bool,
}

impl Target {
    /// The key that tells two reconciler runs apart.
    fn id(&self) -> (&SessionKind, &str, &str) {
        (&self.session, &self.display, &self.user)
    }
}

/// What `loginctl show-session` said about the seat0 session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub user: String,
    pub uid: Option<u32>,
    pub class: String,
    pub session_type: String,
    pub display: String,
    pub locked_hint: String,
}

impl SessionInfo {
    pub fn is_greeter(&self) -> bool {
        self.class == "greeter" || GREETERS.contains(&self.user.as_str())
    }
    pub fn is_x11(&self) -> bool {
        self.session_type.starts_with("x11")
    }
    pub fn is_wayland(&self) -> bool {
        self.session_type == "wayland"
    }
}

fn field(v: Option<&str>) -> String {
    v.unwrap_or_default().trim().to_owned()
}

/// Parses the values of `loginctl show-session … -p Name -p UID -p Class -p Type -p Display -p LockedHint` in
/// that order (the caller asks for those properties), or `None` when the first value is empty.
pub fn parse_session(values: &[&str]) -> Option<SessionInfo> {
    let user = field(values.first().copied());
    if user.is_empty() {
        return None;
    }
    Some(SessionInfo {
        id: String::new(),
        user,
        uid: field(values.get(1).copied()).parse().ok(),
        class: field(values.get(2).copied()),
        session_type: field(values.get(3).copied()),
        display: field(values.get(4).copied()),
        locked_hint: field(values.get(5).copied()),
    })
}

/// The console session state from logind facts: a greeter is the login screen, `LockedHint=yes` the lock screen,
/// any other active session is logged in. `None` (no logind, no seat) is the login screen on a Device with no one.
pub fn state_from(session: Option<&SessionInfo>) -> SessionState {
    match session {
        None => SessionState::LoginScreen,
        Some(s) if s.is_greeter() => SessionState::LoginScreen,
        Some(s) if s.locked_hint == "yes" => SessionState::Locked,
        Some(_) => SessionState::LoggedIn,
    }
}

/// Where a display manager keeps its X server's cookie, newest first, for `display` (`:0`, `:1`).
fn greeter_auths(root: &Path, display: &str) -> Vec<PathBuf> {
    let base = |p: &str| root.join(p.trim_start_matches('/'));
    let mut out = vec![
        base(&format!("/run/lightdm/root/{display}")),
        base(&format!("/var/run/lightdm/root/{display}")),
        base("/run/gdm/authority"),
        base("/run/gdm3/authority"),
        base("/var/lib/gdm/.Xauthority"),
        base("/var/lib/gdm3/.Xauthority"),
    ];
    // SDDM names its file after a random suffix.
    for dir in [base("/run/sddm"), base("/var/run/sddm")] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("xauth")))
                .collect();
            found.sort();
            out.extend(found);
        }
    }
    // A numbered file for another display is not this display's cookie.
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// The X server's `-auth` argument from `/proc/*/cmdline`, for `display`; the most reliable source for both the
/// greeter (root-owned cookie) and a user session (`~/.Xauthority`, or `/run/user/<uid>/gdm/Xauthority`).
pub fn xorg_auth(root: &Path, display: &str) -> Option<PathBuf> {
    let proc = root.join("proc");
    for entry in std::fs::read_dir(&proc).ok()?.flatten() {
        let Ok(bytes) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let args: Vec<&str> = bytes.split(|b| *b == 0).filter_map(|a| std::str::from_utf8(a).ok()).collect();
        let Some(name) = args.first() else { continue };
        let exe = name.rsplit('/').next().unwrap_or(name);
        if !(exe.starts_with('X') || exe.starts_with("Xorg")) {
            continue;
        }
        // Xorg argv: /usr/lib/xorg/Xorg :0 -seat seat0 -auth /run/lightdm/root/:0 …
        let on_display = args.contains(&display);
        let names_a_display = args.iter().any(|a| a.starts_with(':'));
        let auth = args.iter().position(|a| *a == "-auth").and_then(|i| args.get(i + 1)).map(Path::new);
        if let Some(auth) = auth.filter(|a| a.is_file() && (on_display || !names_a_display)) {
            return Some(auth.to_owned());
        }
    }
    None
}

/// Known auth files for a normal user session, in order of preference.
fn user_auths(root: &Path, user: &str, uid: Option<u32>) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(uid) = uid {
        out.push(root.join(format!("run/user/{uid}/gdm/Xauthority")));
        out.push(root.join(format!("run/user/{uid}/.Xauthority")));
    }
    out.push(root.join(format!("home/{user}/.Xauthority")));
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// The X11 display from `/tmp/.X11-unix/X<N>` when logind did not tell us (an old greeter, a bare Xvfb).
fn socket_display(root: &Path) -> Option<String> {
    let mut nums: Vec<u32> = std::fs::read_dir(root.join("tmp/.X11-unix"))
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_prefix('X')?.parse().ok())
        .collect();
    nums.sort_unstable();
    nums.first().map(|n| format!(":{n}"))
}

/// The auth file for `session`: the X server's own `-auth`, then the display manager's paths, then the user's.
pub fn target_for(root: &Path, session: &SessionInfo, display: &str) -> Option<Target> {
    let xauthority = xorg_auth(root, display)
        .or_else(|| if session.is_greeter() { greeter_auths(root, display).into_iter().next() } else { None })
        .or_else(|| user_auths(root, &session.user, session.uid).into_iter().next())?;
    Some(Target {
        session: SessionKind::X11,
        display: display.to_owned(),
        user: session.user.clone(),
        uid: session.uid,
        xauthority: Some(xauthority),
        greeter: session.is_greeter(),
    })
}

/// The Wayland target of `session`: there is no X cookie, only the compositor's socket in the user's runtime dir.
pub fn wayland_target(session: &SessionInfo, display: String) -> Target {
    Target {
        session: SessionKind::Wayland,
        display,
        user: session.user.clone(),
        uid: session.uid,
        xauthority: None,
        greeter: session.is_greeter(),
    }
}

/// The compositor's Wayland socket for `uid`: `wayland-0` when there is one, else the first `wayland-*` entry
/// (a nested compositor uses `wayland-1` and up; the lock files are not sockets).
pub fn wayland_display(root: &Path, uid: Option<u32>) -> Option<String> {
    let uid = uid?;
    let mut found: Vec<String> = std::fs::read_dir(root.join(format!("run/user/{uid}")))
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().into_owned().into())
        .filter(|name: &String| name.starts_with("wayland-") && !name.contains(".lock"))
        .collect();
    found.sort();
    found.iter().find(|name| *name == "wayland-0").cloned().or_else(|| found.first().cloned())
}

/// Reads the seat0 session through `loginctl` (`run` is a seam: it returns a property value).
pub fn seat_session(run: &dyn Fn(&[&str]) -> Option<String>) -> Option<SessionInfo> {
    let id = run(&["show-seat", "seat0", "-p", "ActiveSession", "--value"])?;
    let id = id.trim();
    if id.is_empty() || id == "-" {
        return None;
    }
    let values = |props: &[&str]| -> Option<String> {
        let mut args = vec!["show-session", id];
        for p in props {
            args.push("-p");
            args.push(p);
        }
        args.push("--value");
        run(&args)
    };
    let text = values(&["Name", "UID", "Class", "Type", "Display", "LockedHint"])?;
    let lines: Vec<&str> = text.lines().collect();
    let mut session = parse_session(&lines)?;
    session.id = id.to_owned();
    Some(session)
}

/// Whether `uid` owns the console session (a greeter or the logged-in user): only such a helper is trusted.
pub fn console_uid(run: &dyn Fn(&[&str]) -> Option<String>) -> Option<u32> {
    seat_session(run).and_then(|s| s.uid)
}

/// The graphical session on the console that a helper (or the GUI window) can be put into, if any.
fn target_at(root: &Path, loginctl: &dyn Fn(&[&str]) -> Option<String>) -> Option<Target> {
    let session = seat_session(loginctl)?;
    if session.is_wayland() {
        // A Wayland greeter (GNOME's login screen) has no screen-cast portal to share its screen with: there is
        // nothing a helper could capture, so report "unavailable" instead of a dead helper.
        if session.is_greeter() {
            return None;
        }
        return Some(wayland_target(&session, wayland_display(root, session.uid)?));
    }
    let display = match session.display.is_empty() {
        true => socket_display(root)?,
        false => session.display.clone(),
    };
    target_for(root, &session, &display)
}

/// `target_at` on this machine: the root service uses it to open the GUI window in the console session.
pub fn console_target() -> Option<Target> {
    target_at(Path::new("/"), &loginctl)
}

/// The real `loginctl` reader: `None` when it fails or prints nothing.
fn loginctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("loginctl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The `VAR=value` pairs that put a process into `target`'s graphical session.
pub fn session_env(target: &Target) -> Vec<String> {
    match target.session {
        SessionKind::X11 => vec![
            format!("DISPLAY={}", target.display),
            format!("XAUTHORITY={}", target.xauthority.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
        ],
        SessionKind::Wayland => {
            let mut env = vec!["XDG_SESSION_TYPE=wayland".to_owned(), format!("WAYLAND_DISPLAY={}", target.display)];
            if let Some(uid) = target.uid {
                env.push(format!("XDG_RUNTIME_DIR=/run/user/{uid}"));
                env.push(format!("DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus"));
            }
            env
        }
    }
}

/// `runuser -u <user> -- env <session variables> <exe> agent session --socket <socket>`.
///
/// X11 gets `DISPLAY`/`XAUTHORITY`; Wayland gets the compositor socket, the runtime dir (which also holds the
/// session bus) and the stored portal restore token, so the helper does not prompt again.
pub fn helper_args(exe: &Path, socket: &Path, target: &Target, restore_token: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = vec!["-u".into(), target.user.clone(), "--".into(), "env".into()];
    args.extend(session_env(target));
    if let (SessionKind::Wayland, Some(token)) = (&target.session, restore_token) {
        args.push(format!("{TOKEN_ENV}={token}"));
    }
    args.extend([
        exe.display().to_string(),
        "agent".into(),
        "session".into(),
        "--socket".into(),
        socket.display().to_string(),
    ]);
    args
}

/// How the reconciler starts a helper (with the stored portal restore token); tests replace it (the root-only
/// `runuser` cannot run in a test process).
pub type Spawn = dyn Fn(&Target, Option<&str>) -> anyhow::Result<Child> + Send + Sync;
type Loginctl = dyn Fn(&[&str]) -> Option<String> + Send + Sync;

/// The Agent service's Platform: screen calls go to the helper for seat0, everything else to `inner`.
pub struct ConsoleSession {
    inner: Arc<dyn Platform>,
    sessions: Arc<SessionPlatform>,
    /// The helper's process and the Target it serves, so a login/logout replaces it.
    running: Mutex<Option<(Target, Child)>>,
    /// uids a helper may connect as (root, and the session user the Agent service just spawned).
    trusted: Arc<Mutex<Vec<u32>>>,
    root: PathBuf,
    loginctl: Arc<Loginctl>,
    spawn: Arc<Spawn>,
    /// The portal restore token in the Device state dir; the helper gets it at spawn and publishes new ones.
    token: Option<TokenStore>,
    /// Wayland input: a virtual device only root can create, written here and not in the helper.
    uinput: Mutex<Option<VirtualInput>>,
}

impl ConsoleSession {
    /// The installed root service: `/`, the real `loginctl`/`runuser` and the running binary.
    pub fn start(inner: Arc<dyn Platform>, socket: &Path, state_dir: Option<&Path>) -> anyhow::Result<Arc<Self>> {
        let exe = std::env::current_exe()?;
        Self::start_with(inner, socket, "/", Arc::new(loginctl), None, &exe, state_dir)
    }

    /// `start` with the host root and the seams given: `spawn` (`None` = `runuser`) and `loginctl`.
    pub fn start_with(
        inner: Arc<dyn Platform>,
        socket: &Path,
        root: impl Into<PathBuf>,
        loginctl: Arc<Loginctl>,
        spawn: Option<Arc<Spawn>>,
        exe: &Path,
        state_dir: Option<&Path>,
    ) -> anyhow::Result<Arc<Self>> {
        let trusted: Arc<Mutex<Vec<u32>>> = Default::default();
        let trusted_for_listener = trusted.clone();
        // The greeter and the lock screen both accept input on Linux (XTest, or uinput on Wayland), so the login
        // screen is not special.
        let sessions = SessionPlatform::listen_with(
            inner.clone(),
            socket,
            move |uid| uid == 0 || trusted_for_listener.lock().unwrap().contains(&uid),
            true,
        )?;
        let token = state_dir.map(|dir| TokenStore::new(dir.join(super::screencast::TOKEN_FILE)));
        // A helper publishes the portal's new restore token over the wire; the Agent service owns the state dir.
        if let Some(store) = token.clone() {
            sessions.on_restore_token(move |token| {
                // Every state poll carries the token; only a new one is worth a write.
                if store.load().as_deref() == Some(token) {
                    return;
                }
                if let Err(e) = store.save(token) {
                    tracing::warn!("cannot store the screen cast restore token: {e:#}");
                }
            });
        }
        let spawn: Arc<Spawn> = spawn.unwrap_or_else(|| {
            let (exe, socket) = (exe.to_owned(), socket.to_owned());
            Arc::new(move |target, token| spawn_runuser(&exe, &socket, target, token))
        });
        Ok(Arc::new(Self {
            inner,
            sessions,
            running: Mutex::new(None),
            trusted,
            root: root.into(),
            loginctl,
            spawn,
            token,
            uinput: Mutex::new(None),
        }))
    }

    fn target(&self) -> Option<Target> {
        target_at(&self.root, self.loginctl.as_ref())
    }

    /// Why there is no helper for the session on the console. A Wayland session is served now, so the difference is
    /// between "nothing is logged in" (X11 wording, also what a headless Device reports) and a Wayland greeter.
    fn no_helper_error(&self) -> anyhow::Error {
        match seat_session(self.loginctl.as_ref()) {
            Some(session) if session.is_wayland() && session.is_greeter() => anyhow::anyhow!(
                "screen capture at a Wayland login screen is not available: the greeter has no screen-cast portal"
            ),
            Some(session) if session.is_wayland() => anyhow::anyhow!(
                "the Wayland session on the console did not start its capture helper (is the screen-cast portal running?)"
            ),
            _ => anyhow::anyhow!("no X11 session on the console: cannot see the screen"),
        }
    }

    /// The live session state from logind, independent of whether a helper is up (a Wayland session has none).
    fn login_state(&self) -> SessionState {
        state_from(seat_session(self.loginctl.as_ref()).as_ref())
    }

    /// Starts, replaces or leaves alone the helper for the session on seat0. A login, logout or display-manager
    /// restart changes the target, so the old helper is killed and a new one started.
    fn reconcile(&self) {
        let target = self.target();
        let mut running = self.running.lock().unwrap();
        let same = match (&target, running.as_mut()) {
            (Some(target), Some((current, child))) => current.id() == target.id() && child_is_running(child),
            _ => false,
        };
        if same {
            return;
        }
        if let Some((_, mut child)) = running.take() {
            stop(&mut child);
            self.sessions.clear_helpers();
        }
        // Only the session on the console may drive the screen: forget the previous user as well as its helper.
        let Some(target) = target else {
            self.trusted.lock().unwrap().clear();
            return;
        };
        let token = self.token.as_ref().and_then(TokenStore::load);
        {
            let mut trusted = self.trusted.lock().unwrap();
            trusted.clear();
            if let Some(uid) = target.uid {
                trusted.push(uid);
            }
        }
        match (self.spawn)(&target, token.as_deref()) {
            Ok(child) => *running = Some((target, child)),
            Err(e) => tracing::warn!("cannot start the session helper: {e:#}"),
        }
    }

    /// Makes sure a helper is up for the current console session (waiting briefly for it to connect).
    fn ensure_helper(&self) -> anyhow::Result<()> {
        if self.sessions.helper_count() > 0 {
            return Ok(());
        }
        self.reconcile();
        let start = Instant::now();
        while self.sessions.helper_count() == 0 {
            anyhow::ensure!(start.elapsed() < Duration::from_secs(5), self.no_helper_error());
            std::thread::sleep(Duration::from_millis(50));
            self.reconcile();
        }
        Ok(())
    }

    fn check<T>(&self, f: impl FnOnce(&SessionPlatform) -> anyhow::Result<T>) -> anyhow::Result<T> {
        self.ensure_helper()?;
        f(&self.sessions)
    }

    fn seat_is_wayland(&self) -> bool {
        seat_session(self.loginctl.as_ref()).is_some_and(|session| session.is_wayland())
    }

    /// Writes `input` to the Agent service's virtual device; a move needs the portal's display geometry.
    fn uinput_input(&self, input: Input) -> anyhow::Result<()> {
        let displays = match input {
            Input::Move { .. } => self.sessions.displays()?,
            _ => vec![],
        };
        let mut device = self.uinput.lock().unwrap();
        if device.is_none() {
            *device = Some(VirtualInput::create()?);
        }
        let out = device.as_mut().expect("just created").input(input, &displays);
        if out.is_err() {
            *device = None;
        }
        out
    }
}

impl Platform for ConsoleSession {
    fn hostname(&self) -> String {
        self.inner.hostname()
    }
    fn displays(&self) -> anyhow::Result<Vec<crate::platform::DisplayInfo>> {
        self.check(|s| s.displays())
    }
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        self.check(|s| s.capture(display))
    }
    fn input(&self, input: Input) -> anyhow::Result<()> {
        if self.seat_is_wayland() {
            // Wayland input is the Agent service's uinput device: the helper runs as the session user, which cannot create
            // one, and the portal has no input path at all. It also reaches a Wayland greeter, which has no portal.
            return self.uinput_input(input);
        }
        // XTest reaches the greeter and the lock screen (unlike CGEvent at the macOS login window).
        self.check(|s| s.input(input))
    }
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        self.check(|s| s.accessibility(display))
    }
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        // OCR runs in the helper (it has the session's environment for tesseract) through capture-side work; the
        // frame is already here, so the Agent service does it. tesseract needs no X.
        self.inner.recognize_text(frame)
    }
    fn session_state(&self) -> anyhow::Result<SessionState> {
        Ok(self.login_state())
    }
    fn inventory(&self) -> anyhow::Result<Inventory> {
        let mut inventory = self.inner.inventory()?;
        // The Agent service has no DISPLAY of its own; take the displays from the console session when it has one.
        if self.sessions.helper_count() > 0 {
            if let Ok(displays) = self.sessions.displays() {
                inventory.displays = displays
                    .into_iter()
                    .map(|d| crate::platform::Display { name: d.index.to_string(), width: d.width, height: d.height })
                    .collect();
            }
        }
        Ok(inventory)
    }
    fn metrics(&self) -> anyhow::Result<Resources> {
        self.inner.metrics()
    }
    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        self.inner.processes()
    }
    fn shell(&self, command: &str, account: Account, timeout: Duration) -> anyhow::Result<ShellOutput> {
        self.inner.shell(command, account, timeout)
    }
    fn pty(&self, account: Account, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        self.inner.pty(account, cols, rows)
    }
    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        if action == PowerAction::Lock {
            // The Agent service has no session of its own; lock the one on the console by id.
            let session = seat_session(self.loginctl.as_ref()).ok_or_else(|| anyhow::anyhow!("nobody is logged in to lock"))?;
            let out = std::process::Command::new("loginctl")
                .args(["lock-session", &session.id])
                .output()?;
            anyhow::ensure!(out.status.success(), "loginctl lock-session failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            return Ok(());
        }
        self.inner.power(action)
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

/// The root service's spawn: `runuser` switches to the session user, which is what makes the helper see that
/// user's X authority and (for the greeter) the display manager's cookie.
fn spawn_runuser(exe: &Path, socket: &Path, target: &Target, token: Option<&str>) -> anyhow::Result<Child> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new("runuser");
    command.args(helper_args(exe, socket, target, token)).process_group(0);
    command.spawn().map_err(|e| anyhow::anyhow!("cannot start the session helper as {}: {e}", target.user))
}

fn child_is_running(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

/// Stops a helper and everything it started (`runuser` and the session process share a process group).
fn stop(child: &mut Child) {
    #[cfg(target_os = "linux")]
    if let Ok(pid) = i32::try_from(child.id()) {
        // runuser execs the helper in the same group; the negative pid reaches both.
        unsafe { libc::kill(-pid, libc::SIGTERM) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Starts the reconciler and the platform for the installed root service; `state_dir` holds the portal token.
pub fn console(inner: Arc<dyn Platform>, socket: &Path, state_dir: Option<&Path>) -> anyhow::Result<Arc<ConsoleSession>> {
    let this = ConsoleSession::start(inner, socket, state_dir)?;
    this.reconcile();
    let reconciler = this.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        reconciler.reconcile();
    });
    Ok(this)
}

/// `rcli agent session` logging helper: its environment already has DISPLAY/XAUTHORITY, so the ordinary adapter works.
pub fn run_helper(socket: &Path) -> ! {
    let platform = match crate::linux::LinuxPlatform::new("session") {
        Ok(platform) => platform,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "cannot start the session helper: {e:#}");
            std::process::exit(1);
        }
    };
    crate::macos_session::run_helper(socket, &platform, &|| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcli-linux-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn a_greeter_session_is_the_login_screen_and_a_locked_user_is_the_lock_screen() {
        let greeter = SessionInfo {
            id: "c1".into(),
            user: "lightdm".into(),
            uid: Some(111),
            class: "greeter".into(),
            session_type: "x11".into(),
            display: ":0".into(),
            locked_hint: "no".into(),
        };
        assert_eq!(state_from(Some(&greeter)), SessionState::LoginScreen);
        assert!(greeter.is_greeter() && greeter.is_x11());
        let user = SessionInfo { user: "alice".into(), class: "user".into(), locked_hint: "yes".into(), ..greeter.clone() };
        assert_eq!(state_from(Some(&user)), SessionState::Locked);
        assert_eq!(state_from(Some(&SessionInfo { locked_hint: "no".into(), ..user })), SessionState::LoggedIn);
        assert_eq!(state_from(None), SessionState::LoginScreen);
    }

    #[test]
    fn loginctl_values_are_parsed_in_the_property_order() {
        let parsed = parse_session(&["alice", "1000", "user", "x11", ":1", "no"]).unwrap();
        assert_eq!(
            parsed,
            SessionInfo {
                id: String::new(),
                user: "alice".into(),
                uid: Some(1000),
                class: "user".into(),
                session_type: "x11".into(),
                display: ":1".into(),
                locked_hint: "no".into(),
            }
        );
        assert!(parse_session(&["", "1000"]).is_none());
    }

    #[test]
    fn the_target_uses_the_seat_session_display_and_the_x_servers_auth_file() {
        let root = root("target");
        touch(&root.join("proc/812/cmdline"));
        std::fs::write(
            root.join("proc/812/cmdline"),
            b"/usr/lib/xorg/Xorg\0:0\0-seat\0seat0\0-auth\0/run/lightdm/root/:0\0-nolisten\0tcp\0",
        )
        .unwrap();
        touch(&root.join("run/lightdm/root/:0"));
        let run = |args: &[&str]| -> Option<String> {
            match args {
                ["show-seat", "seat0", "-p", "ActiveSession", "--value"] => Some("c1\n".into()),
                ["show-session", "c1", ..] => Some("lightdm\n111\ngreeter\nx11\n:0\nno\n".into()),
                _ => None,
            }
        };
        let session = seat_session(&run).unwrap();
        assert_eq!((session.id.as_str(), session.user.as_str(), session.is_greeter()), ("c1", "lightdm", true));
        let target = target_for(&root, &session, &session.display).unwrap();
        assert_eq!(target.xauthority, Some(root.join("run/lightdm/root/:0")));
        assert!(target.greeter);
        assert_eq!(console_uid(&run), Some(111));

        // LightDM's root-only cookie is found without Xorg's process too.
        std::fs::remove_dir_all(root.join("proc")).unwrap();
        assert_eq!(target_for(&root, &session, ":0").unwrap().xauthority, Some(root.join("run/lightdm/root/:0")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_user_session_uses_the_gdm_xauthority_under_run_user_or_the_home_directory() {
        let root = root("user");
        touch(&root.join("run/user/1000/gdm/Xauthority"));
        let session = SessionInfo {
            id: "2".into(),
            user: "alice".into(),
            uid: Some(1000),
            class: "user".into(),
            session_type: "x11".into(),
            display: ":1".into(),
            locked_hint: "no".into(),
        };
        assert_eq!(
            target_for(&root, &session, ":1").unwrap().xauthority,
            Some(root.join("run/user/1000/gdm/Xauthority"))
        );
        // No Xorg cmdline, no greeter paths: the home directory's cookie is the next choice.
        std::fs::remove_dir_all(root.join("run")).unwrap();
        touch(&root.join("home/alice/.Xauthority"));
        assert_eq!(target_for(&root, &session, ":1").unwrap().xauthority, Some(root.join("home/alice/.Xauthority")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_display_falls_back_to_the_x11_socket_and_no_x11_session_has_no_target() {
        let root = root("display");
        touch(&root.join("tmp/.X11-unix/X1"));
        assert_eq!(socket_display(&root).as_deref(), Some(":1"));
        let session = SessionInfo {
            id: "2".into(),
            user: "alice".into(),
            uid: Some(1000),
            class: "user".into(),
            session_type: "wayland".into(),
            display: String::new(),
            locked_hint: "no".into(),
        };
        // A Wayland session exposes no Xauthority worth using; no cookie means no target (screenshot fails clearly).
        assert!(target_for(&root, &session, ":1").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    fn wayland_session() -> SessionInfo {
        SessionInfo {
            id: "2".into(),
            user: "alice".into(),
            uid: Some(1000),
            class: "user".into(),
            session_type: "wayland".into(),
            display: String::new(),
            locked_hint: "no".into(),
        }
    }

    /// The Agent service with a scripted logind; the helper is never spawned (`spawn` would fail in a test process).
    fn agent_service(root: &Path, session: SessionInfo) -> (Arc<ConsoleSession>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rcli-linux-console-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("session.sock");
        let run = Arc::new(move |args: &[&str]| -> Option<String> {
            match args {
                ["show-seat", "seat0", "-p", "ActiveSession", "--value"] => Some("2\n".into()),
                ["show-session", "2", ..] => Some(format!(
                    "{}\n{}\n{}\n{}\n{}\n{}\n",
                    session.user,
                    session.uid.unwrap_or(0),
                    session.class,
                    session.session_type,
                    session.display,
                    session.locked_hint
                )),
                _ => None,
            }
        });
        let spawn: Arc<Spawn> = Arc::new(|_, _| anyhow::bail!("no helper in a test"));
        let this = ConsoleSession::start_with(
            Arc::new(crate::platform::FakePlatform::new("daemon")),
            &socket,
            root,
            run,
            Some(spawn),
            Path::new("/usr/local/lib/rcli/rcli"),
            None,
        )
        .unwrap();
        (this, dir)
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn a_wayland_session_targets_the_compositors_socket_and_has_no_x_cookie() {
        let root = root("wayland");
        touch(&root.join("run/user/1000/wayland-1"));
        touch(&root.join("run/user/1000/wayland-0"));
        touch(&root.join("run/user/1000/wayland-0.lock"));
        // The compositor's own socket wins over a nested one, and lock files are not sockets.
        assert_eq!(wayland_display(&root, Some(1000)).as_deref(), Some("wayland-0"));
        std::fs::remove_file(root.join("run/user/1000/wayland-0")).unwrap();
        assert_eq!(wayland_display(&root, Some(1000)).as_deref(), Some("wayland-1"));
        assert_eq!(wayland_display(&root, Some(1001)), None);
        let target = wayland_target(&wayland_session(), "wayland-0".into());
        assert_eq!((target.session, target.xauthority), (SessionKind::Wayland, None));
        assert!(!target.greeter);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_wayland_helper_gets_the_session_bus_the_socket_and_the_stored_token() {
        let target = wayland_target(&wayland_session(), "wayland-0".into());
        let args = helper_args(Path::new("/usr/local/lib/rcli/rcli"), Path::new("/run/rcli/session.sock"), &target, Some("tok-1"));
        assert_eq!(
            args,
            vec![
                "-u",
                "alice",
                "--",
                "env",
                "XDG_SESSION_TYPE=wayland",
                "WAYLAND_DISPLAY=wayland-0",
                "XDG_RUNTIME_DIR=/run/user/1000",
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
                "RCLI_SCREENCAST_RESTORE_TOKEN=tok-1",
                "/usr/local/lib/rcli/rcli",
                "agent",
                "session",
                "--socket",
                "/run/rcli/session.sock",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
        // Without a token the helper asks the portal once; the environment is otherwise the same.
        let args = helper_args(Path::new("/usr/local/lib/rcli/rcli"), Path::new("/run/rcli/session.sock"), &target, None);
        assert!(!args.iter().any(|arg| arg.starts_with("RCLI_SCREENCAST_RESTORE_TOKEN=")), "{args:?}");
        assert!(!args.iter().any(|arg| arg.starts_with("XAUTHORITY=")), "a Wayland session has no X cookie: {args:?}");
    }

    #[test]
    fn the_reconciler_serves_a_logged_in_wayland_session_and_refuses_its_greeter() {
        let root = root("wayland-console");
        touch(&root.join("run/user/1000/wayland-0"));
        let session = wayland_session();
        let (console, dir) = agent_service(&root, session.clone());
        let target = console.target().expect("a logged-in Wayland session can be served now");
        assert_eq!((target.session, target.display.as_str()), (SessionKind::Wayland, "wayland-0"));
        assert!(console.seat_is_wayland(), "input then goes through uinput, not the helper");
        let _ = std::fs::remove_dir_all(dir);

        let greeter = SessionInfo { user: "gdm".into(), uid: Some(111), class: "greeter".into(), ..session };
        let (greeter_service, greeter_dir) = agent_service(&root, greeter);
        assert!(greeter_service.target().is_none(), "the greeter has no screen-cast portal to share");
        assert!(
            greeter_service.no_helper_error().to_string().contains("Wayland login screen"),
            "{}",
            greeter_service.no_helper_error()
        );
        let _ = std::fs::remove_dir_all(greeter_dir);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_helper_runs_as_the_session_user_with_the_displays_environment() {
        let target = Target {
            session: SessionKind::X11,
            display: ":1".into(),
            user: "lightdm".into(),
            uid: Some(111),
            xauthority: Some(PathBuf::from("/run/lightdm/root/:1")),
            greeter: true,
        };
        assert_eq!(
            helper_args(Path::new("/usr/local/lib/rcli/rcli"), Path::new("/run/rcli/session.sock"), &target, None),
            vec![
                "-u",
                "lightdm",
                "--",
                "env",
                "DISPLAY=:1",
                "XAUTHORITY=/run/lightdm/root/:1",
                "/usr/local/lib/rcli/rcli",
                "agent",
                "session",
                "--socket",
                "/run/rcli/session.sock",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
    }
}
