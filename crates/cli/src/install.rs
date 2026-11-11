//! Small helpers shared by the per-OS `rcli install` implementations: the options every install takes, the
//! command seam, the rooted path, the file writer, the Device Settings an install records, the Terminal User
//! fallback, and the registration an install starts with and an uninstall undoes.

use std::path::{Path, PathBuf};

use rcli_agent::settings::{DeviceSettings, RootAccess};

/// Runs an external program (`systemctl`, `launchctl`, `sc.exe`, `icacls.exe`, `ditto`); a seam for tests.
pub type Run<'a> = &'a mut dyn FnMut(&str, &[&str]) -> anyhow::Result<()>;

/// Runs `program` for real; the command must succeed.
pub fn system(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let out = std::process::Command::new(program).args(args).output()?;
    anyhow::ensure!(out.status.success(), "{program} {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

/// What `rcli install` was given, on every platform.
pub struct Options {
    pub server: String,
    pub key: String,
    pub name: String,
    /// The Terminal User, already resolved (`--terminal-user`, else the installing or console account).
    pub terminal_user: String,
    pub allow_root: bool,
}

/// The Terminal User an install records when `--terminal-user` is absent: the account that ran `sudo`, else the
/// account running the install. `root` is not a Terminal User when a person is available.
pub fn terminal_user(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("SUDO_USER").ok())
        .filter(|u| !u.trim().is_empty() && u != "root")
        .unwrap_or_else(rcli_agent::settings::default_terminal_user)
}

/// `path` under a `root`-prefixed layout, so tests point every installed path at a temp directory.
pub fn at(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// Writes `contents` to `path` with `mode`, creating the parent directory.
#[cfg(unix)]
pub fn write(path: &Path, contents: &str, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Records the Device Settings an install was asked for: the Terminal User, and root always with `--allow-root`.
pub fn save_settings(state: &Path, opts: &Options) -> anyhow::Result<()> {
    let mut settings = DeviceSettings::load(state);
    settings.terminal_user = opts.terminal_user.clone();
    if opts.allow_root {
        settings.root = RootAccess::Always;
    }
    settings.save(state)
}

/// This Device's credentials as the Agent saved them in `state`, if it is registered.
pub fn device_credentials(state: &Path) -> Option<rcli_agent::Credentials> {
    serde_json::from_slice(&std::fs::read(state.join("credentials.json")).ok()?).ok()
}

/// Registers this Device with the Server the install was given (or keeps its existing registration), so a wrong
/// Server Key fails before the service starts, then writes the machine-wide Controller configuration at `machine`
/// (ADR 0006). Returns the Device ID and Device Password lines the install prints.
pub async fn register(
    opts: &Options,
    platform: &dyn rcli_agent::platform::Platform,
    state: &Path,
    machine: &Path,
) -> anyhow::Result<String> {
    let server = rcli_agent::ServerAccess { addr: opts.server.clone(), server_key: opts.key.clone() };
    let secrets = crate::agent::secret_store(state);
    let creds = crate::agent::load_or_register(&server, platform, state, secrets.as_ref()).await?;
    let password = secrets.get(rcli_agent::ipc::DEVICE_PASSWORD)?.unwrap_or_default();
    // A reinstall keeps its saved registration, which does not carry the API Address: keep the one already known.
    let known = crate::config::load_machine(machine).ok().flatten().map(|m| m.api_address).unwrap_or_default();
    let api_address = if creds.api_address.is_empty() { known } else { creds.api_address.clone() };
    let config = crate::config::Machine {
        server: opts.server.clone(),
        server_key: opts.key.clone(),
        update_key: hex::encode(&creds.update_key),
        api_address,
    };
    crate::config::save_machine(machine, &config)?;
    let mut text = format!("Device ID: {}\nDevice Password: {password}\n", creds.device_id);
    if config.api_address.is_empty() {
        text += "The Server has no API Address yet, so `rcli connect` and `rcli mcp` wait until an Admin opens the \
                 Dashboard over https:// once.\n";
    }
    Ok(text)
}

/// The Device's state directory under `root` (the `--root` test seam): credentials, Device Settings, Secret Store.
pub fn state_dir(root: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    return crate::macos_install::Layout { root: root.into() }.state_dir();
    #[cfg(target_os = "linux")]
    return crate::linux_install::Layout { root: root.into() }.state_dir();
    #[cfg(windows)]
    return crate::windows_install::Layout::under(root).state_dir();
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    root.join("rcli")
}

/// An uninstall's first steps: removes the Device from the Server at `server` (the Server Address the install
/// recorded), then deletes its secrets. An unreachable Server is a warning, not a failure: the local install must
/// still be removable, and an Admin can delete the Device from the Dashboard.
pub async fn unregister(server: Option<String>, state: &Path, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    if let (Some(server), Some(creds)) = (server, device_credentials(state)) {
        if let Err(e) = rcli_agent::unregister_at(&server, &creds).await {
            writeln!(out, "warning: could not remove the Device from the Server: {e:#}")?;
        }
    }
    let secrets = crate::agent::secret_store(state);
    if let Err(e) = secrets.remove(rcli_agent::ipc::DEVICE_PASSWORD) {
        writeln!(out, "warning: {e:#}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_user_is_the_flag_first_then_the_installing_account() {
        assert_eq!(terminal_user(Some("alice".into())), "alice");
        // Without the flag: the account that ran `sudo` when there is one, else this process's account.
        let from_sudo = std::env::var("SUDO_USER").ok().filter(|u| !u.trim().is_empty() && u != "root");
        match from_sudo {
            Some(user) => assert_eq!(terminal_user(None), user),
            None => assert_eq!(terminal_user(None), rcli_agent::settings::default_terminal_user()),
        }
        // An explicit root is not a Terminal User; the fallback decides, so it equals the no-flag answer (which is
        // still "root" when root is the account running the install, as in a container).
        assert_eq!(terminal_user(Some("root".into())), terminal_user(None));
    }

    #[test]
    fn paths_hang_off_the_layout_root() {
        let root = Path::new("/tmp/rcli-root");
        assert_eq!(at(root, "/usr/local/lib/rcli/rcli"), root.join("usr/local/lib/rcli/rcli"));
        assert_eq!(at(root, "relative"), root.join("relative"));
    }
}
