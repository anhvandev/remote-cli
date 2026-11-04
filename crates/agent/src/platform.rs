//! The Platform port: the only way the Agent core touches the OS.
//! OS adapters (tickets 28-35) and the FakePlatform implement it. Add methods here as Agent features need them.

pub trait Platform: Send + Sync + 'static {
    fn hostname(&self) -> String;
}

/// Scripted Platform for tests: no OS access.
#[derive(Clone, Debug)]
pub struct FakePlatform {
    pub hostname: String,
}

impl FakePlatform {
    pub fn new(hostname: impl Into<String>) -> Self {
        Self { hostname: hostname.into() }
    }
}

impl Platform for FakePlatform {
    fn hostname(&self) -> String {
        self.hostname.clone()
    }
}
