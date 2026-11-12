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
    /// One-shot command through the OS shell, as `account`. Kills it after `timeout` and returns what it printed
    /// so far with `timed_out` set. `Account::User` always names an account; `sudo` inside works as over SSH.
    fn shell(&self, command: &str, account: Account, timeout: Duration) -> anyhow::Result<ShellOutput>;
    /// Interactive login shell on a new PTY of `cols` x `rows`, as `account` (same rule as `shell`).
    /// Full PTY semantics: input goes to the terminal as typed (Ctrl-C = 0x03 interrupts), TERM supports full-screen programs.
    fn pty(&self, account: Account, cols: u16, rows: u16) -> anyhow::Result<Pty>;
    fn power(&self, action: PowerAction) -> anyhow::Result<()>;
    /// Sends a Wake-on-LAN magic packet for `mac` on the local network.
    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()>;
    /// Version of the running Agent.
    fn agent_version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }
    /// Replaces the Agent binary with the file at `build` (signature already verified) and restarts the Agent
    /// onto it. Real adapters normally do not return on success; if they do, the Agent reconnects. An adapter that
    /// does not return deletes `build` once the binary is swapped; otherwise the caller deletes it.
    fn install_update(&self, version: &str, build: &std::path::Path) -> anyhow::Result<()>;
    /// Shows (true) or hides the visible on-Device indicator that someone is controlling it
    /// (Control Lock held or live stream active). Must work at the lock and login screens too.
    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()>;
    /// Asks a person at the Device whether `requester` may run as root, with Accept/Reject, for at most `timeout`.
    /// `true` = Accept; `false` = Reject or no answer in time. Only called while someone is logged in.
    fn ask_root(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool>;
    /// Asks a person at the Device whether to accept a new connection from `requester`, with Accept/Reject, for at
    /// most `timeout`. `true` = Accept; `false` = Reject or no answer in time. Only called while someone is logged in.
    fn ask_accept(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool>;
    /// Whether Live View may try a hardware H.264 encoder before openh264. Real adapters let it try: one that is
    /// missing or cannot start falls back by itself. The FakePlatform can say no, like a machine without a GPU.
    fn hardware_encoder(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// An on-screen element a Mark is made from. Box in real pixels of its display.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UiElement {
    pub role: String,
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One input primitive. The Executor builds clicks, drags and key combinations from these.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Ctrl+Alt+Del. Windows cannot synthesize it with SendInput: the SYSTEM session helper calls `SendSAS`
    /// instead (the software-SAS policy must allow it, see `docs/usage.md`). Unavailable elsewhere.
    Sas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    pub timed_out: bool,
}

/// A running PTY session. `output` reaches EOF when the shell has exited.
pub struct Pty {
    pub output: Box<dyn std::io::Read + Send>,
    pub input: Box<dyn std::io::Write + Send>,
    pub control: Box<dyn PtyControl>,
}

pub trait PtyControl: Send {
    fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()>;
    /// Exit code of the shell, waiting for it to end (called after `output` hit EOF).
    fn wait(&mut self) -> anyhow::Result<i32>;
    /// Ends the shell and everything it started.
    fn kill(&mut self) -> anyhow::Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Account {
    /// The Terminal User: the account Device Settings say commands and shells run as.
    User(String),
    /// root, or SYSTEM on Windows.
    Root,
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
    powers: Vec<PowerAction>,
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
    /// Every capture differs from the previous one by a little (below what counts as a change), like a blinking caret.
    pub flicker: bool,
    /// Every capture is fresh noise (8x8 blocks of random colour) instead of one colour, like a playing video:
    /// what a bitrate limit bites on.
    pub noise: bool,
    /// Whether Live View may use a hardware encoder; false forces openh264 (a VM without a GPU).
    pub hardware_encoder: bool,
    /// Root prompts shown, in order: who asked, and the timeout the Agent allowed.
    pub root_prompts: Vec<(String, Duration)>,
    /// The scripted person's answers: true = Accept, false = Reject. An empty queue = nobody answers, which
    /// auto-rejects; the FakePlatform answers at once instead of waiting out `timeout`.
    pub root_answers: std::collections::VecDeque<bool>,
    /// Accept-connection prompts shown, in order: who connects, and the timeout the Agent allowed.
    pub accept_prompts: Vec<(String, Duration)>,
    /// The scripted person's answers to accept-connection prompts, like `root_answers`.
    pub accept_answers: std::collections::VecDeque<bool>,
    /// While true, `install_update` waits (a slow update); `installing` is true while it is in there.
    pub hold_install: bool,
    pub installing: bool,
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
            powers: vec![],
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
            flicker: false,
            noise: false,
            hardware_encoder: true,
            root_prompts: vec![],
            root_answers: Default::default(),
            accept_prompts: vec![],
            accept_answers: Default::default(),
            hold_install: false,
            installing: false,
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

    /// Every power Action received, in order.
    pub fn power_actions(&self) -> Vec<PowerAction> {
        self.state.lock().unwrap().powers.clone()
    }

    /// Whether the control indicator is currently shown.
    pub fn control_indicator(&self) -> bool {
        self.state.lock().unwrap().control_indicator
    }

    /// Versions installed through `install_update`, in order.
    pub fn installed_updates(&self) -> Vec<String> {
        self.state.lock().unwrap().installed.clone()
    }

    /// Every root prompt shown, in order: who asked, and the timeout it was given.
    pub fn root_prompts(&self) -> Vec<(String, Duration)> {
        self.state.lock().unwrap().root_prompts.clone()
    }

    /// Scripts the person's answers, in order; anything left scripted answers later prompts. With none left,
    /// the person does not answer and the prompt auto-rejects.
    pub fn answer_root_prompts(&self, answers: impl IntoIterator<Item = bool>) {
        self.state.lock().unwrap().root_answers = answers.into_iter().collect();
    }

    /// Every accept-connection prompt shown, in order: who connects, and the timeout it was given.
    pub fn accept_prompts(&self) -> Vec<(String, Duration)> {
        self.state.lock().unwrap().accept_prompts.clone()
    }

    /// Scripts the person's answers to accept-connection prompts, like `answer_root_prompts`.
    pub fn answer_accept_prompts(&self, answers: impl IntoIterator<Item = bool>) {
        self.state.lock().unwrap().accept_answers = answers.into_iter().collect();
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
        // Steps of 40 so every change is noticeable to `frames`.
        let flicker = if s.flicker { (s.captures % 2 * 8) as u8 } else { 0 };
        let pixel = [s.session as u8 * 60 + flicker, (s.inputs.len() * 40) as u8, (frame_no % 6 * 40) as u8, 255];
        let mut rgba = pixel.repeat((d.width * d.height) as usize);
        if s.noise {
            for (i, p) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (i as u64 % d.width as u64 / 8, i as u64 / d.width as u64 / 8);
                // Hash of (capture, 8x8 block): a fresh random colour per block and capture.
                let h = (s.captures << 40 ^ y << 20 ^ x).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                p[..3].copy_from_slice(&h.to_be_bytes()[..3]);
            }
        }
        Ok(Frame { width: d.width, height: d.height, rgba })
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

    fn shell(&self, command: &str, account: Account, timeout: Duration) -> anyhow::Result<ShellOutput> {
        Ok(fake_shell(command, account, timeout))
    }

    fn pty(&self, account: Account, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        let state = FakePtyState { output: b"$ ".to_vec(), size: (cols, rows), ..Default::default() };
        let shared = Arc::new((Mutex::new(state), std::sync::Condvar::new()));
        let pty = FakePty { shared, account };
        Ok(Pty { output: Box::new(pty.clone()), input: Box::new(pty.clone()), control: Box::new(pty) })
    }

    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        self.state.lock().unwrap().powers.push(action);
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
    fn install_update(&self, version: &str, build: &std::path::Path) -> anyhow::Result<()> {
        anyhow::ensure!(build.is_file(), "no build at {}", build.display());
        self.state.lock().unwrap().installing = true;
        while self.state.lock().unwrap().hold_install {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut s = self.state.lock().unwrap();
        s.installing = false;
        s.installed.push(version.into());
        s.version = version.into();
        Ok(())
    }

    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()> {
        self.state.lock().unwrap().control_indicator = on;
        Ok(())
    }

    /// Scripted: records the prompt (who asked, the timeout the Agent allowed) and answers at once from
    /// `root_answers`; an empty queue is the person not answering before the timeout, which rejects.
    fn ask_root(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        let mut s = self.state.lock().unwrap();
        s.root_prompts.push((requester.to_owned(), timeout));
        Ok(s.root_answers.pop_front().unwrap_or(false))
    }

    /// Scripted like `ask_root`, for accepting a new connection.
    fn ask_accept(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        let mut s = self.state.lock().unwrap();
        s.accept_prompts.push((requester.to_owned(), timeout));
        Ok(s.accept_answers.pop_front().unwrap_or(false))
    }

    fn hardware_encoder(&self) -> bool {
        self.state.lock().unwrap().hardware_encoder
    }
}

// shortcut: only `echo X`, `whoami` ("user" or "root") and `sleep SECS` are emulated; anything else exits 127.
fn fake_shell(command: &str, account: Account, timeout: Duration) -> ShellOutput {
    let ok = |stdout: String| ShellOutput { exit_code: 0, stdout, stderr: String::new(), timed_out: false };
    if let Some(secs) = command.strip_prefix("sleep ").and_then(|s| s.parse().ok()) {
        let wanted = Duration::from_secs(secs);
        std::thread::sleep(wanted.min(timeout));
        return ShellOutput { timed_out: wanted > timeout, ..ok(String::new()) };
    }
    match (command, command.strip_prefix("echo ")) {
        (_, Some(text)) => ok(format!("{text}\n")),
        ("whoami", _) => ok(match &account {
            Account::User(user) => format!("{user}\n"),
            Account::Root => "root\n".into(),
        }),
        // `sudo` inside the terminal works as over SSH: the command then runs as root.
        _ => match command.strip_prefix("sudo ") {
            Some(inner) => fake_shell(inner, Account::Root, timeout),
            None => ShellOutput { exit_code: 127, stdout: String::new(), stderr: format!("{command}: not found\n"), timed_out: false },
        },
    }
}

/// Fake terminal: echoes input, runs each line through `fake_shell` with prompt `$ `, plus `stty size`
/// (prints "ROWS COLS"), `exit [N]`, Ctrl-C (prints `^C` and a new prompt) and Ctrl-D on an empty line (exit 0).
#[derive(Clone)]
struct FakePty {
    shared: Arc<(Mutex<FakePtyState>, std::sync::Condvar)>,
    account: Account,
}

#[derive(Default)]
struct FakePtyState {
    output: Vec<u8>,
    line: String,
    size: (u16, u16),
    exit: Option<i32>,
}

impl FakePty {
    fn update<T>(&self, f: impl FnOnce(&mut FakePtyState) -> T) -> T {
        let out = f(&mut self.shared.0.lock().unwrap());
        self.shared.1.notify_all();
        out
    }
}

impl std::io::Read for FakePty {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let (lock, cvar) = &*self.shared;
        let mut s = cvar.wait_while(lock.lock().unwrap(), |s| s.output.is_empty() && s.exit.is_none()).unwrap();
        let n = buf.len().min(s.output.len());
        buf[..n].copy_from_slice(&s.output[..n]);
        s.output.drain(..n);
        Ok(n)
    }
}

impl std::io::Write for FakePty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let account = self.account.clone();
        self.update(|s| {
            for &b in buf {
                if s.exit.is_some() {
                    break;
                }
                match b {
                    3 => {
                        s.line.clear();
                        s.output.extend_from_slice(b"^C\r\n$ ");
                    }
                    4 if s.line.is_empty() => s.exit = Some(0),
                    b'\r' | b'\n' => {
                        s.output.extend_from_slice(b"\r\n");
                        let line = std::mem::take(&mut s.line);
                        let text = match line.trim() {
                            "" => String::new(),
                            "stty size" => format!("{} {}\n", s.size.1, s.size.0),
                            l if l == "exit" || l.starts_with("exit ") => {
                                s.exit = Some(l[4..].trim().parse().unwrap_or(0));
                                break;
                            }
                            l => {
                                let out = fake_shell(l, account.clone(), Duration::ZERO);
                                out.stdout + &out.stderr
                            }
                        };
                        s.output.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
                        s.output.extend_from_slice(b"$ ");
                    }
                    b => {
                        s.line.push(b as char);
                        s.output.push(b);
                    }
                }
            }
        });
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl PtyControl for FakePty {
    fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.update(|s| s.size = (cols, rows));
        Ok(())
    }

    fn wait(&mut self) -> anyhow::Result<i32> {
        let (lock, cvar) = &*self.shared;
        Ok(cvar.wait_while(lock.lock().unwrap(), |s| s.exit.is_none()).unwrap().exit.unwrap())
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        self.update(|s| {
            s.exit.get_or_insert(-9);
        });
        Ok(())
    }
}
