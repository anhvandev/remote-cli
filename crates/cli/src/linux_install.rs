//! `rcli install|uninstall` on Linux (ticket 16): the binary in /usr/local/lib/rcli, a root systemd
//! service that comes back after a reboot, and the Device's Device Settings. The Server Key travels in a
//! root-only `EnvironmentFile`, never on the command line (`ps` shows every process's arguments to every user).
//! Every path hangs off `Layout::root` and every external command goes through `run`, so the layout, the unit
//! file and the systemctl pipeline are tested without touching the Device.

use std::path::{Path, PathBuf};

use crate::install::{system, write};
pub use crate::install::{Options, Run};

pub const UNIT: &str = "rcli-agent.service";
const KEY_ENV: &str = "RCLI_SERVER_KEY";
/// The app-menu entry `rcli install` places; running it opens the GUI window (ticket 14).
const DESKTOP: &str = include_str!("../../../packaging/linux/rcli.desktop");

pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    fn at(&self, path: &str) -> PathBuf {
        crate::install::at(&self.root, path)
    }
    /// The stable path the service runs; `rcli update` replaces this file in place.
    pub fn exe(&self) -> PathBuf {
        self.at("/usr/local/lib/rcli/rcli")
    }
    /// `rcli` on the PATH for the terminal.
    pub fn link(&self) -> PathBuf {
        self.at("/usr/local/bin/rcli")
    }
    /// Credentials, Device Settings and the Secret Store. `StateDirectory=` recreates it as root 0700.
    pub fn state_dir(&self) -> PathBuf {
        self.at("/var/lib/rcli")
    }
    pub fn unit(&self) -> PathBuf {
        self.at(&format!("/etc/systemd/system/{UNIT}"))
    }
    /// Holds the Server Key; root-only.
    pub fn env_file(&self) -> PathBuf {
        self.at("/etc/rcli/agent.env")
    }
    /// Where the service records the Server Address for `uninstall` (no secret: the Key is in `env_file`).
    pub fn install_file(&self) -> PathBuf {
        self.state_dir().join("install.json")
    }
    /// Where the root service listens for the console session helper.
    pub fn session_socket(&self) -> PathBuf {
        self.at("/run/rcli/session.sock")
    }
    /// The IPC socket `rcli agent …`, the GUI and scripts use (the `agent run` default).
    pub fn ipc_socket(&self) -> PathBuf {
        self.at("/run/rcli/agent.sock")
    }
    /// The app-menu entry: running `rcli` with no arguments opens the window.
    pub fn desktop_file(&self) -> PathBuf {
        self.at("/usr/share/applications/rcli.desktop")
    }
    /// The machine-wide Controller configuration, readable by every local user (the state directory is root-only).
    pub fn controller_config(&self) -> PathBuf {
        crate::config::machine_path(Some(&self.root))
    }
}

/// Quotes one argument for a systemd unit file: bare when simple, else double-quoted with systemd's escapes.
fn systemd_arg(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@+-".contains(c)) {
        return s.to_owned();
    }
    let mut escaped = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' | '"' => {
                escaped.push('\\');
                escaped.push(c);
            }
            // `%` starts a systemd specifier in Exec lines; `%%` is a literal one.
            '%' => escaped.push_str("%%"),
            _ => escaped.push(c),
        }
    }
    format!("\"{escaped}\"")
}

/// The root service: the Agent itself. `--session-socket` makes the Agent service spawn and supervise the console
/// session helper (see `rcli_agent::linux::session`). The Server Key comes from `EnvironmentFile`, and the unit
/// names that file, so the key is never an argument.
pub fn unit(layout: &Layout, opts: &Options) -> String {
    let exe = systemd_arg(&layout.exe().display().to_string());
    let state = systemd_arg(&layout.state_dir().display().to_string());
    let name = systemd_arg(&opts.name);
    let server = systemd_arg(&opts.server);
    let session = systemd_arg(&layout.session_socket().display().to_string());
    let env = systemd_arg(&layout.env_file().display().to_string());
    let ipc = systemd_arg(&layout.ipc_socket().display().to_string());
    format!(
        "[Unit]\n\
         Description=rcli Agent\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User=root\n\
         EnvironmentFile={env}\n\
         Environment=RCLI_AGENT_SOCKET={ipc}\n\
         ExecStart={exe} agent run --server {server} --state-dir {state} --name {name} --session-socket {session}\n\
         Restart=always\n\
         RestartSec=2\n\
         # The IPC and session sockets live here, 0755 so local users can reach `rcli agent info` and disconnect.\n\
         RuntimeDirectory=rcli\n\
         RuntimeDirectoryMode=0755\n\
         StateDirectory=rcli\n\
         StateDirectoryMode=0700\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

/// The root-only `EnvironmentFile`: one `RCLI_SERVER_KEY=…` line. systemd parses it shell-like; the Server Key's
/// alphabet (`rk_`, hex, `_`) needs no quoting.
pub fn environment_file(opts: &Options) -> String {
    format!("{KEY_ENV}={}\n", opts.key)
}

/// Copies the running binary to the stable path, links it, records the Device Settings and the Server Address, and
/// installs the app-menu entry.
/// `exe` is the running binary (the download, the deb payload or the RPM payload).
pub fn install_files(layout: &Layout, opts: &Options, exe: &Path, _run: Run) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = layout.exe();
    if exe != target {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Copy beside the target and rename, so a running service never sees a half-written binary.
        let temp = target.with_extension("new");
        std::fs::copy(exe, &temp)?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&temp, &target)?;
    }
    let link = layout.link();
    std::fs::create_dir_all(link.parent().unwrap())?;
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&target, &link)?;

    let state = layout.state_dir();
    std::fs::create_dir_all(&state)?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))?;
    crate::install::save_settings(&state, opts)?;
    // The app menu launches the GUI window: `rcli` with no arguments (ticket 14).
    write(&layout.desktop_file(), DESKTOP, 0o644)?;
    write(&layout.install_file(), &serde_json::json!({ "server": opts.server }).to_string(), 0o600)
}

/// Writes the unit and the root-only key file, then enables and (re)starts the service. `restart` also covers an
/// upgrade: it starts a first install and replaces the running binary's process on a reinstall.
pub fn install_service(layout: &Layout, opts: &Options, run: Run) -> anyhow::Result<()> {
    write(&layout.unit(), &unit(layout, opts), 0o644)?;
    write(&layout.env_file(), &environment_file(opts), 0o600)?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", UNIT])?;
    run("systemctl", &["restart", UNIT])
}

/// Stops and removes the service, the binary, the key file and the Device's local data (credentials, settings and
/// secrets). The caller has already removed the Device from the Server.
pub fn uninstall_files(layout: &Layout, run: Run) -> anyhow::Result<()> {
    // A missing unit is not an error: a first install that failed after copying files must still be removable.
    let _ = run("systemctl", &["disable", "--now", UNIT]);
    for file in [layout.unit(), layout.env_file(), layout.session_socket(), layout.link(), layout.desktop_file(), layout.controller_config()] {
        let _ = std::fs::remove_file(file);
    }
    // `disable --now` stops it; the reload forgets the removed unit.
    let _ = run("systemctl", &["daemon-reload"]);
    if layout.state_dir().exists() {
        std::fs::remove_dir_all(layout.state_dir())?;
    }
    if layout.exe().exists() {
        std::fs::remove_file(layout.exe())?;
    }
    for dir in [layout.exe().parent().map(Path::to_path_buf), layout.env_file().parent().map(Path::to_path_buf)] {
        if let Some(dir) = dir.filter(|d| d.exists()) {
            // Only if empty: /usr/local/lib may hold other programs.
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(())
}

/// Reads the Server Address `install_files` recorded, for `uninstall` (no secret; the Device's certificate comes
/// from `credentials.json`).
fn recorded_server(layout: &Layout) -> Option<String> {
    let text = std::fs::read_to_string(layout.install_file()).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?["server"].as_str().map(str::to_owned)
}

/// `sudo rcli install`: files, Device Settings, registration and the machine-wide Controller configuration, then the
/// systemd service. Registering first makes a wrong Server Key fail at the install and lets the Device ID and Device
/// Password be printed.
pub async fn install(opts: Options, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli install --server … --key …");
    let platform = rcli_agent::linux::LinuxPlatform::new(opts.name.clone())?;
    install_at(&Layout { root: "/".into() }, opts, &platform, &mut system, out).await
}

/// `install` on `layout`, registering with `platform`'s inventory; the `--root` test seam calls it directly.
pub async fn install_at(
    layout: &Layout,
    opts: Options,
    platform: &dyn rcli_agent::platform::Platform,
    run: Run<'_>,
    out: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    // A reinstall must not destroy a working install when the new Server Key is wrong.
    crate::install::check_server_key(&opts).await?;
    let reinstall = layout.state_dir().join("credentials.json").exists();
    install_files(layout, &opts, &std::env::current_exe()?, run)?;
    let registered = match crate::install::register(&opts, platform, &layout.state_dir(), &layout.controller_config()).await {
        Ok(registered) => registered,
        Err(e) => {
            if !reinstall {
                let _ = uninstall_files(layout, run);
            }
            return Err(e.context(crate::install::REGISTER_FAILED));
        }
    };
    install_service(layout, &opts, run)?;
    write!(
        out,
        "installed {UNIT} and {}\n{registered}\
         The root service captures the lock screen and the display-manager greeter over X11; a Wayland \
         greeter or session is not visible until it runs an X11 session. On a headless server, metrics, \
         exec, shell and power work without a display.\n",
        layout.exe().display(),
    )?;
    if !rcli_agent::secret_store::systemd_creds_available() {
        writeln!(
            out,
            "warning: systemd-creds is unavailable, so the Device Password stays in the root-only encrypted file {}",
            layout.state_dir().display()
        )?;
    }
    Ok(())
}

/// `sudo rcli uninstall`: removes the Device from the Server (see `install::unregister`), then stops and removes the
/// service and deletes the Device's local data and the machine-wide Controller configuration.
pub async fn uninstall(out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli uninstall");
    uninstall_at(&Layout { root: "/".into() }, &mut system, out).await
}

pub async fn uninstall_at(layout: &Layout, run: Run<'_>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    crate::install::unregister(recorded_server(layout), &layout.state_dir(), out).await?;
    uninstall_files(layout, run)?;
    writeln!(out, "removed {UNIT}, {} and the Device's local data", layout.exe().display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcli_agent::settings::{DeviceSettings, RootAccess};

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcli-linux-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn opts() -> Options {
        Options {
            server: "rcli.example.com:7443".into(),
            key: "rk_ab12_s3cr&t".into(),
            name: "linux-box".into(),
            terminal_user: "alice".into(),
            allow_root: true,
        }
    }

    fn recorder(calls: &mut Vec<String>) -> impl FnMut(&str, &[&str]) -> anyhow::Result<()> + '_ {
        move |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        }
    }

    #[test]
    fn install_puts_the_binary_unit_key_file_and_settings_in_place_and_uninstall_removes_them() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("roundtrip");
        let layout = Layout { root: root.clone() };
        let exe = root.join("download/rcli");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"binary").unwrap();
        let mut calls = vec![];

        install_files(&layout, &opts(), &exe, &mut recorder(&mut calls)).unwrap();
        install_service(&layout, &opts(), &mut recorder(&mut calls)).unwrap();

        assert_eq!(std::fs::read(root.join("usr/local/lib/rcli/rcli")).unwrap(), b"binary");
        assert_eq!(std::fs::read_link(root.join("usr/local/bin/rcli")).unwrap(), layout.exe());
        let settings = DeviceSettings::load(&layout.state_dir());
        assert_eq!((settings.terminal_user.as_str(), settings.root), ("alice", RootAccess::Always));
        assert_eq!(std::fs::metadata(layout.state_dir()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(layout.env_file()).unwrap().permissions().mode() & 0o777, 0o600, "it holds the Server Key");
        assert_eq!(std::fs::metadata(layout.install_file()).unwrap().permissions().mode() & 0o777, 0o600);
        // The app menu launches the window with no arguments; `rcli install` puts the entry there.
        let desktop = std::fs::read_to_string(layout.desktop_file()).unwrap();
        assert!(desktop.contains("Exec=/usr/local/bin/rcli\n"), "{desktop}");
        assert!(desktop.contains("[Desktop Entry]") && desktop.contains("Terminal=false"), "{desktop}");
        assert_eq!(std::fs::metadata(layout.desktop_file()).unwrap().permissions().mode() & 0o777, 0o644);
        let env = std::fs::read_to_string(layout.env_file()).unwrap();
        assert_eq!(env, "RCLI_SERVER_KEY=rk_ab12_s3cr&t\n");
        let unit = std::fs::read_to_string(layout.unit()).unwrap();
        assert!(unit.contains(&format!("EnvironmentFile={}", layout.env_file().display())), "{unit}");
        assert!(unit.contains(&format!("ExecStart={} agent run", layout.exe().display())), "{unit}");
        assert!(unit.contains(&format!("--session-socket {}", layout.session_socket().display())), "{unit}");
        assert!(unit.contains("Restart=always") && unit.contains("WantedBy=multi-user.target"), "{unit}");
        assert_eq!(recorded_server(&layout).as_deref(), Some("rcli.example.com:7443"));
        assert_eq!(
            calls,
            vec![
                "systemctl daemon-reload".to_owned(),
                "systemctl enable rcli-agent.service".to_owned(),
                "systemctl restart rcli-agent.service".to_owned()
            ]
        );

        calls.clear();
        uninstall_files(&layout, &mut recorder(&mut calls)).unwrap();
        assert_eq!(
            calls,
            vec!["systemctl disable --now rcli-agent.service".to_owned(), "systemctl daemon-reload".to_owned()]
        );
        for gone in [
            "usr/local/lib/rcli/rcli",
            "usr/local/bin/rcli",
            "var/lib/rcli",
            "etc/systemd/system/rcli-agent.service",
            "etc/rcli/agent.env",
            "usr/share/applications/rcli.desktop",
        ] {
            assert!(!root.join(gone).exists(), "{gone} is left behind");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_unit_runs_the_agent_as_root_with_the_key_out_of_its_arguments() {
        let layout = Layout { root: "/".into() };
        let unit = unit(&layout, &opts());
        let expected = "ExecStart=/usr/local/lib/rcli/rcli agent run --server rcli.example.com:7443 \
                        --state-dir /var/lib/rcli --name linux-box --session-socket /run/rcli/session.sock\n";
        assert!(unit.contains(expected), "{unit}");
        assert!(!unit.contains("rk_"), "the Server Key is never on a command line: {unit}");
        assert!(unit.contains("StateDirectoryMode=0700"), "{unit}");
    }

    #[test]
    fn a_name_with_spaces_and_specifiers_is_quoted_and_escaped() {
        let layout = Layout { root: "/".into() };
        let mut o = opts();
        o.name = "bench %1 \"box\"".into();
        let unit = unit(&layout, &o);
        assert!(unit.contains("--name \"bench %%1 \\\"box\\\"\" --session-socket"), "{unit}");
    }
}
