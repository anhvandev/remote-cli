//! Small helpers shared by the per-OS `rcli agent install` implementations: the options every install takes, the
//! command seam, the rooted path, the file writer, the Device Settings an install records and the Terminal User
//! fallback.

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

/// What `rcli agent install` was given, on every platform.
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
        // An explicit root is not a Terminal User; the fallback takes over.
        assert_ne!(terminal_user(Some("root".into())), "root");
    }

    #[test]
    fn paths_hang_off_the_layout_root() {
        let root = Path::new("/tmp/rcli-root");
        assert_eq!(at(root, "/usr/local/lib/rcli/rcli"), root.join("usr/local/lib/rcli/rcli"));
        assert_eq!(at(root, "relative"), root.join("relative"));
    }
}
