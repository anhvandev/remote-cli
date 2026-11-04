//! The Platform port: the only way the Agent core touches the OS.
//! OS adapters (tickets 28-35) and the FakePlatform implement it; `contract` checks them. Add methods here as Agent features need them.

use std::sync::{Arc, Mutex};

pub use rcli_protocol::agent::{Disk, Display, Inventory, NetworkInterface, Process, Resources};

pub trait Platform: Send + Sync + 'static {
    fn hostname(&self) -> String;
    /// One frame of the primary display.
    fn capture(&self) -> anyhow::Result<Frame>;
    /// Presses and releases one key, e.g. `"a"`.
    fn key(&self, key: &str) -> anyhow::Result<()>;
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
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
    keys: Vec<String>,
    wakes: Vec<[u8; 6]>,
    version: String,
    installed: Vec<String>,
    pub inventory: Inventory,
    pub resources: Resources,
    pub processes: Vec<Process>,
}

impl FakePlatform {
    pub fn new(hostname: impl Into<String>) -> Self {
        let hostname = hostname.into();
        let state = FakeState {
            session: SessionState::LoggedIn,
            keys: vec![],
            wakes: vec![],
            inventory: Inventory { os: "FakeOS".into(), hostname: hostname.clone(), cpu_cores: 4, ..Default::default() },
            resources: Resources { cpu_percent: Some(12.5), ram_used_bytes: Some(4 << 30), ..Default::default() },
            processes: vec![Process { pid: 1, name: "init".into(), ..Default::default() }],
            version: env!("CARGO_PKG_VERSION").into(),
            installed: vec![],
        };
        Self { hostname, state: Arc::new(Mutex::new(state)) }
    }

    pub fn update(&self, f: impl FnOnce(&mut FakeState)) {
        f(&mut self.state.lock().unwrap());
    }

    pub fn set_session_state(&self, session: SessionState) {
        self.state.lock().unwrap().session = session;
    }

    pub fn keys_pressed(&self) -> Vec<String> {
        self.state.lock().unwrap().keys.clone()
    }

    pub fn wakes_sent(&self) -> Vec<[u8; 6]> {
        self.state.lock().unwrap().wakes.clone()
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

    /// A 2x2 frame that changes with the session state and every key pressed.
    fn capture(&self) -> anyhow::Result<Frame> {
        let s = self.state.lock().unwrap();
        let mut rgba = vec![0; 16];
        rgba[0] = s.session as u8;
        rgba[1] = s.keys.len() as u8;
        Ok(Frame { width: 2, height: 2, rgba })
    }

    fn key(&self, key: &str) -> anyhow::Result<()> {
        self.state.lock().unwrap().keys.push(key.into());
        Ok(())
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
}
