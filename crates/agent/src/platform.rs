//! The Platform port: the only way the Agent core touches the OS.
//! OS adapters (tickets 28-35) and the FakePlatform implement it; `contract` checks them. Add methods here as Agent features need them.

use std::sync::{Arc, Mutex};

pub trait Platform: Send + Sync + 'static {
    fn hostname(&self) -> String;
    /// One frame of the primary display.
    fn capture(&self) -> anyhow::Result<Frame>;
    /// Presses and releases one key, e.g. `"a"`.
    fn key(&self, key: &str) -> anyhow::Result<()>;
    fn session_state(&self) -> anyhow::Result<SessionState>;
    fn metrics(&self) -> anyhow::Result<Metrics>;
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

#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
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

/// Scripted Platform for tests: no OS access. Clones share state, so a test can keep one and script the screen.
#[derive(Clone, Debug)]
pub struct FakePlatform {
    pub hostname: String,
    state: Arc<Mutex<FakeState>>,
}

#[derive(Debug)]
struct FakeState {
    session: SessionState,
    keys: Vec<String>,
    wakes: Vec<[u8; 6]>,
    version: String,
    installed: Vec<String>,
}

impl FakePlatform {
    pub fn new(hostname: impl Into<String>) -> Self {
        let state = FakeState { session: SessionState::LoggedIn, keys: vec![], wakes: vec![], version: env!("CARGO_PKG_VERSION").into(), installed: vec![] };
        Self { hostname: hostname.into(), state: Arc::new(Mutex::new(state)) }
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

    fn metrics(&self) -> anyhow::Result<Metrics> {
        Ok(Metrics { cpu_percent: 12.5, ram_used_bytes: 4 << 30 })
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
