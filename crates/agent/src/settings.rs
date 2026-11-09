//! Device Settings: the rules the person at the Device sets, enforced by the Agent for every Controller.
//! Kept in `<state dir>/settings.json`; a change applies to the running Agent at once.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `sudo rcli agent root never|ask|always`: what `--as root` may do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootAccess {
    Never,
    /// Ask a person at the Device; rejected when no session can show the prompt.
    #[default]
    Ask,
    Always,
}

impl RootAccess {
    pub fn name(self) -> &'static str {
        match self {
            RootAccess::Never => "never",
            RootAccess::Ask => "ask",
            RootAccess::Always => "always",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSettings {
    /// The account terminal commands and shells run as.
    pub terminal_user: String,
    #[serde(default)]
    pub root: RootAccess,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self { terminal_user: default_terminal_user(), root: RootAccess::default() }
    }
}

/// The installing account. Until the installers record `SUDO_USER` (tickets 16/22/24), that is this account.
pub fn default_terminal_user() -> String {
    std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "root".into())
}

impl DeviceSettings {
    /// `settings.json` under `dir`; a missing or unreadable file means the defaults.
    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("settings.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

/// The live Device Settings of a running Agent, plus where they are saved.
#[derive(Debug)]
pub struct Handle {
    dir: std::sync::Mutex<Option<PathBuf>>,
    settings: std::sync::Mutex<DeviceSettings>,
}

impl Handle {
    pub fn new() -> Self {
        Self { dir: std::sync::Mutex::new(None), settings: std::sync::Mutex::new(DeviceSettings::default()) }
    }

    /// Settings kept in `dir`; loaded now and, once a later `set_dir` says so, again.
    pub fn with_dir(dir: PathBuf) -> Self {
        let handle = Self::new();
        handle.set_dir(dir);
        handle
    }

    pub fn set_dir(&self, dir: PathBuf) {
        *self.settings.lock().unwrap() = DeviceSettings::load(&dir);
        *self.dir.lock().unwrap() = Some(dir);
    }

    pub fn get(&self) -> DeviceSettings {
        self.settings.lock().unwrap().clone()
    }

    /// Applies a change and saves it; with no directory (a test Agent without one) it stays in memory.
    pub fn update(&self, f: impl FnOnce(&mut DeviceSettings)) -> anyhow::Result<DeviceSettings> {
        let mut settings = self.settings.lock().unwrap();
        f(&mut settings);
        if let Some(dir) = self.dir.lock().unwrap().as_deref() {
            settings.save(dir)?;
        }
        Ok(settings.clone())
    }
}

impl Default for Handle {
    fn default() -> Self {
        Self::new()
    }
}
