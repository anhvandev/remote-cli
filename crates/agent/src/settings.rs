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

/// `sudo rcli agent accept password|password+accept`: whether a person at the Device must accept each new connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcceptMode {
    /// A Controller with the Device Password connects at once.
    #[default]
    #[serde(rename = "password")]
    Password,
    /// Each new connection is accepted at the Device; rejected on timeout or when no session can show the prompt.
    #[serde(rename = "password+accept")]
    PasswordAccept,
}

impl AcceptMode {
    pub fn name(self) -> &'static str {
        match self {
            AcceptMode::Password => "password",
            AcceptMode::PasswordAccept => "password+accept",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSettings {
    /// The account terminal commands and shells run as.
    pub terminal_user: String,
    #[serde(default)]
    pub root: RootAccess,
    #[serde(default)]
    pub accept: AcceptMode,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self { terminal_user: default_terminal_user(), root: RootAccess::default(), accept: AcceptMode::default() }
    }
}

/// The installing account. The installers record the Terminal User (`--terminal-user`, else `SUDO_USER`; tickets
/// 16/22/24), so this is only the fallback for a Device whose settings were never written.
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

    /// The Device Settings in an `info` reply. The Agent includes them only for root, so a reply without them means
    /// the caller must elevate (the GUI) rather than fall back to defaults.
    pub fn from_info(reply: &serde_json::Value) -> Result<Self, String> {
        let settings = reply.get("settings").ok_or_else(|| {
            "the Device Settings are root-only; read them elevated (`sudo rcli agent info --json`)".to_owned()
        })?;
        serde_json::from_value(settings.clone()).map_err(|e| format!("the info reply has no usable settings: {e}"))
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("settings.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

/// The Device Settings fields the person actually changed. The GUI sends only these, so an editor opened before its
/// values were loaded (or one left open while another change lands) can never rewrite a rule nobody touched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub terminal_user: Option<String>,
    pub root: Option<RootAccess>,
    pub accept: Option<AcceptMode>,
}

impl Change {
    /// What differs between the Settings the Agent last reported and the person's edits.
    pub fn between(current: &DeviceSettings, edited: &DeviceSettings) -> Self {
        Self {
            terminal_user: (current.terminal_user != edited.terminal_user).then(|| edited.terminal_user.clone()),
            root: (current.root != edited.root).then_some(edited.root),
            accept: (current.accept != edited.accept).then_some(edited.accept),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terminal_user.is_none() && self.root.is_none() && self.accept.is_none()
    }

    /// Records the changed fields as applied, so the editor compares against what the Agent now enforces.
    pub fn apply(&self, current: &mut DeviceSettings) {
        if let Some(user) = &self.terminal_user {
            current.terminal_user = user.clone();
        }
        if let Some(root) = self.root {
            current.root = root;
        }
        if let Some(accept) = self.accept {
            current.accept = accept;
        }
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

    /// Where the settings are saved: the Agent's state directory.
    pub fn dir(&self) -> Option<PathBuf> {
        self.dir.lock().unwrap().clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(terminal_user: &str, root: RootAccess, accept: AcceptMode) -> DeviceSettings {
        DeviceSettings { terminal_user: terminal_user.into(), root, accept }
    }

    #[test]
    fn settings_come_from_the_root_only_settings_field_of_an_info_reply() {
        let reply = json!({
            "device_id": "123456789",
            "settings": { "terminal_user": "alice", "root": "always", "accept": "password+accept" },
        });
        let loaded = DeviceSettings::from_info(&reply).unwrap();
        assert_eq!(loaded.terminal_user, "alice");
        assert_eq!(loaded.root, RootAccess::Always);
        assert_eq!(loaded.accept, AcceptMode::PasswordAccept);

        // The same reply for a non-root caller has no `settings`: the GUI must ask for elevation, never assume defaults.
        let err = DeviceSettings::from_info(&json!({ "device_id": "123456789" })).unwrap_err();
        assert!(err.contains("root"), "{err}");
        let err = DeviceSettings::from_info(&json!({ "settings": { "root": "always" } })).unwrap_err();
        assert!(err.contains("settings"), "{err}");
    }

    /// `name()` and the `#[serde(rename…)]` next to it are two encodings of one value: a change to either must show
    /// up in both, or `rcli agent root …` and the IPC/wire values disagree (S8).
    #[test]
    fn the_names_match_what_serde_writes() {
        for (level, name) in [(RootAccess::Never, "never"), (RootAccess::Ask, "ask"), (RootAccess::Always, "always")] {
            assert_eq!(level.name(), name);
            assert_eq!(serde_json::to_value(level).unwrap(), serde_json::json!(name));
        }
        for (mode, name) in [(AcceptMode::Password, "password"), (AcceptMode::PasswordAccept, "password+accept")] {
            assert_eq!(mode.name(), name);
            assert_eq!(serde_json::to_value(mode).unwrap(), serde_json::json!(name));
        }
    }

    #[test]
    fn only_the_fields_the_person_changed_are_sent() {
        let loaded = settings("alice", RootAccess::Always, AcceptMode::PasswordAccept);
        assert!(Change::between(&loaded, &loaded).is_empty(), "an untouched editor changes nothing");

        // A stale editor holding the old default accept mode and root must not rewrite either when only the
        // Terminal User was edited.
        let edited = settings("bob", RootAccess::Ask, AcceptMode::Password);
        let change = Change::between(&settings("alice", RootAccess::Ask, AcceptMode::Password), &edited);
        assert_eq!(change.terminal_user.as_deref(), Some("bob"));
        assert_eq!(change.root, None, "a rule the person did not touch is not sent");
        assert_eq!(change.accept, None, "a rule the person did not touch is not sent");

        let change = Change::between(&loaded, &settings("alice", RootAccess::Never, AcceptMode::PasswordAccept));
        assert_eq!(change.root, Some(RootAccess::Never));
        assert_eq!(change.terminal_user, None);
        assert_eq!(change.accept, None);

        let change = Change::between(&loaded, &settings("alice", RootAccess::Always, AcceptMode::Password));
        assert_eq!(change.accept, Some(AcceptMode::Password));
        assert_eq!(change.terminal_user, None);
        assert_eq!(change.root, None);
    }

    #[test]
    fn applying_a_change_updates_only_what_was_sent() {
        let mut loaded = settings("alice", RootAccess::Always, AcceptMode::PasswordAccept);
        let change = Change { terminal_user: Some("bob".into()), root: None, accept: Some(AcceptMode::Password) };
        change.apply(&mut loaded);
        assert_eq!(loaded.terminal_user, "bob");
        assert_eq!(loaded.root, RootAccess::Always, "the untouched root rule stays as the Agent reported it");
        assert_eq!(loaded.accept, AcceptMode::Password);
    }
}
