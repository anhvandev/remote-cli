//! The Platform port: the only way the Agent core touches the OS.
//! OS adapters (tickets 28-35) and the FakePlatform implement it. Add methods here as Agent features need them.

use std::sync::{Arc, Mutex};

pub use rcli_protocol::agent::{Disk, Display, Inventory, NetworkInterface, Process, Resources, SessionState};

pub trait Platform: Send + Sync + 'static {
    fn hostname(&self) -> String;
    /// Hardware and OS facts; polled, and re-sent when they differ from the last ones sent.
    fn inventory(&self) -> Inventory;
    /// A complete sample of current resources; unavailable values (e.g. no battery) are `None`.
    fn resources(&self) -> Resources;
    fn processes(&self) -> Vec<Process>;
}

/// Scripted Platform for tests: no OS access. Clones share state, so a test can
/// change what a running Agent sees with `update`.
#[derive(Clone, Debug)]
pub struct FakePlatform {
    pub hostname: String,
    state: Arc<Mutex<FakeState>>,
}

#[derive(Clone, Debug, Default)]
pub struct FakeState {
    pub inventory: Inventory,
    pub resources: Resources,
    pub processes: Vec<Process>,
}

impl FakePlatform {
    pub fn new(hostname: impl Into<String>) -> Self {
        let hostname = hostname.into();
        let state = FakeState {
            inventory: Inventory {
                os: "FakeOS".into(),
                hostname: hostname.clone(),
                cpu_cores: 4,
                ..Default::default()
            },
            resources: Resources {
                cpu_percent: Some(0.0),
                session: Some(SessionState::Active.into()),
                ..Default::default()
            },
            processes: vec![],
        };
        Self { hostname, state: Arc::new(Mutex::new(state)) }
    }

    pub fn update(&self, f: impl FnOnce(&mut FakeState)) {
        f(&mut self.state.lock().unwrap());
    }
}

impl Platform for FakePlatform {
    fn hostname(&self) -> String {
        self.hostname.clone()
    }

    fn inventory(&self) -> Inventory {
        self.state.lock().unwrap().inventory.clone()
    }

    fn resources(&self) -> Resources {
        self.state.lock().unwrap().resources.clone()
    }

    fn processes(&self) -> Vec<Process> {
        self.state.lock().unwrap().processes.clone()
    }
}
