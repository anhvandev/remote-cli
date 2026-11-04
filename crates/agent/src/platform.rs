//! The Platform port: the only way the Agent core touches the OS.
//! OS adapters (tickets 28-35) and the FakePlatform implement it; `contract` checks them. Add methods here as Agent features need them.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub use rcli_protocol::agent::{DisplayInfo, Disk, Display, Inventory, NetworkInterface, Process, Resources};

pub trait Platform: Send + Sync + 'static {
    fn hostname(&self) -> String;
    /// Displays with their size in real pixels; `index` is what `capture` and `Input::Move` take.
    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>>;
    /// One frame of `display`, in real pixels.
    fn capture(&self, display: u32) -> anyhow::Result<Frame>;
    fn input(&self, input: Input) -> anyhow::Result<()>;
    /// Targetable elements (buttons, fields, links, labelled text) from the accessibility tree of `display`,
    /// in real pixels. `None` = no tree here (lock/login screen, Wayland, games): the Agent falls back to OCR.
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>>;
    /// OCR: lines or words of text in `frame`, boxes in the frame's pixels, role "text".
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>>;
    fn session_state(&self) -> anyhow::Result<SessionState>;
    /// Hardware and OS facts; polled, and re-sent when they differ from the last ones sent.
    fn inventory(&self) -> anyhow::Result<Inventory>;
    /// A complete sample of current resources; unavailable values (e.g. no battery) are `None`.
    /// `session` is left unset: the Agent fills it from `session_state`.
    fn metrics(&self) -> anyhow::Result<Resources>;
    fn processes(&self) -> anyhow::Result<Vec<Process>>;
    /// One-shot command through the OS shell.
    fn shell(&self, command: &str) -> anyhow::Result<ShellOutput>;
    fn power(&self, action: PowerAction) -> anyhow::Result<()>;
    /// Sends a Wake-on-LAN magic packet for `mac` on the local network.
    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()>;
    /// Version of the running Agent.
    fn agent_version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }
    /// Replaces the Agent binary with `build` (signature already verified) and restarts the Agent
    /// onto it. Real adapters normally do not return on success; if they do, the Agent reconnects.
    fn install_update(&self, version: &str, build: &[u8]) -> anyhow::Result<()>;
    /// Shows (true) or hides the visible on-Device indicator that someone is controlling it
    /// (Control Lock held or live stream active). Must work at the lock and login screens too.
    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// An on-screen element a Mark is made from. Box in real pixels of its display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiElement {
    pub role: String,
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One input primitive. The Executor builds clicks, drags and key combinations from these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// Pointer to real pixel (x, y) of `display`.
    Move { display: u32, x: u32, y: u32 },
    Press { button: Button, down: bool },
    /// Wheel ticks at the pointer; positive dy scrolls down, positive dx right.
    Scroll { dx: i32, dy: i32 },
    /// Unicode text, independent of keyboard layout.
    Text(String),
    /// Press and release a key, e.g. `"a"`, `"enter"`.
    Key(String),
    KeyDown(String),
    KeyUp(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    LoggedIn,
    Locked,
    LoginScreen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Restart,
    Shutdown,
    Sleep,
    Lock,
}

/// Scripted Platform for tests: no OS access. Clones share state, so a test can keep one and script the screen,
/// inventory, resources and processes (`update`).
#[derive(Clone, Debug)]
pub struct FakePlatform {
    pub hostname: String,
    state: Arc<Mutex<FakeState>>,
}

#[derive(Debug)]
pub struct FakeState {
    session: SessionState,
    displays: Vec<DisplayInfo>,
    inputs: Vec<Input>,
    wakes: Vec<[u8; 6]>,
    /// After each input the screen changes on every capture for this long.
    animate: Duration,
    animating_until: Option<Instant>,
    captures: u64,
    version: String,
    installed: Vec<String>,
    control_indicator: bool,
    pub inventory: Inventory,
    pub resources: Resources,
    pub processes: Vec<Process>,
    /// What `accessibility` returns for every display.
    pub accessibility: Option<Vec<UiElement>>,
    /// What `recognize_text` "reads" from any frame.
    pub screen_text: Vec<UiElement>,
}

impl FakePlatform {
    pub fn new(hostname: impl Into<String>) -> Self {
        let hostname = hostname.into();
        let display = DisplayInfo { index: 0, x: 0, y: 0, width: 1920, height: 1080, scale: 1.0, primary: true };
        let state = FakeState {
            session: SessionState::LoggedIn,
            displays: vec![display],
            inputs: vec![],
            wakes: vec![],
            animate: Duration::ZERO,
            animating_until: None,
            captures: 0,
            inventory: Inventory { os: "FakeOS".into(), hostname: hostname.clone(), cpu_cores: 4, ..Default::default() },
            resources: Resources { cpu_percent: Some(12.5), ram_used_bytes: Some(4 << 30), ..Default::default() },
            processes: vec![Process { pid: 1, name: "init".into(), ..Default::default() }],
            version: env!("CARGO_PKG_VERSION").into(),
            installed: vec![],
            control_indicator: false,
            accessibility: None,
            screen_text: vec![],
        };
        Self { hostname, state: Arc::new(Mutex::new(state)) }
    }

    pub fn update(&self, f: impl FnOnce(&mut FakeState)) {
        f(&mut self.state.lock().unwrap());
    }

    pub fn set_session_state(&self, session: SessionState) {
        self.state.lock().unwrap().session = session;
    }

    pub fn set_displays(&self, displays: Vec<DisplayInfo>) {
        self.state.lock().unwrap().displays = displays;
    }

    /// Every input received, in order.
    pub fn inputs(&self) -> Vec<Input> {
        self.state.lock().unwrap().inputs.clone()
    }

    /// From now on, every input starts an animation of `d` (the screen changes on every capture).
    pub fn animate_after_input(&self, d: Duration) {
        self.state.lock().unwrap().animate = d;
    }

    pub fn wakes_sent(&self) -> Vec<[u8; 6]> {
        self.state.lock().unwrap().wakes.clone()
    }

    /// Whether the control indicator is currently shown.
    pub fn control_indicator(&self) -> bool {
        self.state.lock().unwrap().control_indicator
    }

    /// Versions installed through `install_update`, in order.
    pub fn installed_updates(&self) -> Vec<String> {
        self.state.lock().unwrap().installed.clone()
    }
}

impl Platform for FakePlatform {
    fn hostname(&self) -> String {
        self.hostname.clone()
    }

    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        Ok(self.state.lock().unwrap().displays.clone())
    }

    /// A display-sized frame in one color, which changes with the session state, every input and while animating.
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        let mut s = self.state.lock().unwrap();
        s.captures += 1;
        let d = s.displays.iter().find(|d| d.index == display).ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
        let animating = s.animating_until.is_some_and(|t| Instant::now() < t);
        let frame_no = if animating { s.captures } else { 0 };
        let pixel = [s.session as u8 * 60, s.inputs.len() as u8, (frame_no % 251) as u8, 255];
        Ok(Frame { width: d.width, height: d.height, rgba: pixel.repeat((d.width * d.height) as usize) })
    }

    fn input(&self, input: Input) -> anyhow::Result<()> {
        let mut s = self.state.lock().unwrap();
        s.inputs.push(input);
        s.animating_until = Some(Instant::now() + s.animate);
        Ok(())
    }

    fn accessibility(&self, _display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        Ok(self.state.lock().unwrap().accessibility.clone())
    }

    // shortcut: returns the scripted text whatever the frame shows; fine while frames are single-colour.
    fn recognize_text(&self, _frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        Ok(self.state.lock().unwrap().screen_text.clone())
    }

    fn session_state(&self) -> anyhow::Result<SessionState> {
        Ok(self.state.lock().unwrap().session)
    }

    fn inventory(&self) -> anyhow::Result<Inventory> {
        Ok(self.state.lock().unwrap().inventory.clone())
    }

    fn metrics(&self) -> anyhow::Result<Resources> {
        Ok(self.state.lock().unwrap().resources.clone())
    }

    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        Ok(self.state.lock().unwrap().processes.clone())
    }

    // shortcut: only `echo` is emulated; script more results when a feature needs them.
    fn shell(&self, command: &str) -> anyhow::Result<ShellOutput> {
        Ok(match command.strip_prefix("echo ") {
            Some(text) => ShellOutput { exit_code: 0, stdout: format!("{text}\n"), stderr: String::new() },
            None => ShellOutput { exit_code: 127, stdout: String::new(), stderr: format!("{command}: not found\n") },
        })
    }

    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        if action == PowerAction::Lock {
            self.set_session_state(SessionState::Locked);
        }
        Ok(())
    }

    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        self.state.lock().unwrap().wakes.push(mac);
        Ok(())
    }

    fn agent_version(&self) -> String {
        self.state.lock().unwrap().version.clone()
    }

    /// "Restarts" onto `version`: the Agent reports it on its next connection.
    fn install_update(&self, version: &str, _build: &[u8]) -> anyhow::Result<()> {
        let mut s = self.state.lock().unwrap();
        s.installed.push(version.into());
        s.version = version.into();
        Ok(())
    }

    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()> {
        self.state.lock().unwrap().control_indicator = on;
        Ok(())
    }
}
