//! `rcli agent install|uninstall` on Linux (ticket 16): the binary in /usr/local/lib/rcli, a root systemd
//! service that comes back after a reboot, and the Device's Device Settings. The Server Key travels in a
//! root-only `EnvironmentFile`, never on the command line (`ps` shows every process's arguments to every user).
//! Every path hangs off `Layout::root` and every external command goes through `run`, so the layout, the unit
//! file and the systemctl pipeline are tested without touching the machine.

use std::path::{Path, PathBuf};

use rcli_agent::settings::{DeviceSettings, RootAccess};

pub const UNIT: &str = "rcli-agent.service";
const KEY_ENV: &str = "RCLI_SERVER_KEY";

/// Runs an external program (`systemctl`, `hostname`); a seam for tests.
pub type Run<'a> = &'a mut dyn FnMut(&str, &[&str]) -> anyhow::Result<()>;

pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
    /// The stable path the service runs; `rcli upgrade` replaces this file in place.
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
    /// The IPC socket `rcli agent …` clients use (the `agent run` default).
    pub fn ipc_socket(&self) -> PathBuf {
        self.at("/run/rcli/agent.sock")
    }
}

/// What `rcli agent install` was given.
pub struct Options {
    pub server: String,
    pub key: String,
    pub name: String,
    /// The Terminal User; `None` = the installing account (`SUDO_USER`).
    pub terminal_user: Option<String>,
    pub allow_root: bool,
}

impl Options {
    /// The Terminal User to record: the flag, else the account that ran `sudo`, else this process's account.
    fn terminal_user(&self) -> String {
        self.terminal_user
            .clone()
            .or_else(|| std::env::var("SUDO_USER").ok())
            .filter(|u| !u.trim().is_empty() && u != "root")
            .unwrap_or_else(rcli_agent::settings::default_terminal_user)
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

/// The root service: the Agent itself. `--session-socket` makes the daemon spawn and supervise the console
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

fn write(path: &Path, contents: &str, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Copies the running binary to the stable path, links it, records the Device Settings and the Server Address.
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
    let mut settings = DeviceSettings::load(&state);
    settings.terminal_user = opts.terminal_user();
    if opts.allow_root {
        settings.root = RootAccess::Always;
    }
    settings.save(&state)?;
    write(&layout.install_file(), &serde_json::json!({ "server": opts.server }).to_string(), 0o600)?;
    // The Secret Store needs a private directory for the file fallback and the systemd-creds files.
    std::fs::create_dir_all(state.join("secrets"))?;
    std::fs::set_permissions(state.join("secrets"), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Writes the unit and the root-only key file, then enables and starts the service (a reinstall replaces it).
pub fn install_service(layout: &Layout, opts: &Options, run: Run) -> anyhow::Result<()> {
    write(&layout.unit(), &unit(layout, opts), 0o644)?;
    write(&layout.env_file(), &environment_file(opts), 0o600)?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", UNIT])
}

/// Stops and removes the service, the binary, the key file and the Device's local data (credentials, settings and
/// secrets). The caller has already removed the Device from the Server.
pub fn uninstall_files(layout: &Layout, run: Run) -> anyhow::Result<()> {
    // A missing unit is not an error: a first install that failed after copying files must still be removable.
    let _ = run("systemctl", &["disable", "--now", UNIT]);
    for file in [layout.unit(), layout.env_file(), layout.session_socket(), layout.link()] {
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

/// The real machine's `run`: the command must succeed.
fn system(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let out = std::process::Command::new(program).args(args).output()?;
    anyhow::ensure!(out.status.success(), "{program} {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

/// Reads the Server Address `install_files` recorded, for `uninstall` (no secret; the Device's certificate comes
/// from `credentials.json`).
fn recorded_server(layout: &Layout) -> Option<String> {
    let text = std::fs::read_to_string(layout.install_file()).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?["server"].as_str().map(str::to_owned)
}

fn device_credentials(layout: &Layout) -> Option<rcli_agent::Credentials> {
    serde_json::from_slice(&std::fs::read(layout.state_dir().join("credentials.json")).ok()?).ok()
}

/// `sudo rcli agent install`: files, Device Settings, registration, then the systemd service. Registering first
/// makes a wrong Server Key fail at the install and lets the Device ID and Device Password be printed.
pub async fn install(opts: Options, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli agent install --server … --key …");
    let layout = Layout { root: "/".into() };
    let exe = std::env::current_exe()?;
    install_files(&layout, &opts, &exe, &mut system)?;
    let server = rcli_agent::ServerEndpoint { addr: opts.server.clone(), server_key: opts.key.clone() };
    let platform = rcli_agent::linux::LinuxPlatform::new(opts.name.clone())?;
    let secrets = crate::agent::secret_store(&layout.state_dir());
    let creds = crate::agent::load_or_register(&server, &platform, &layout.state_dir(), secrets.as_ref()).await?;
    install_service(&layout, &opts, &mut system)?;
    let password = secrets.get(rcli_agent::ipc::DEVICE_PASSWORD)?.unwrap_or_default();
    writeln!(
        out,
        "installed {UNIT} and {} \nDevice ID: {}\nDevice Password: {password}\n\
         The root service captures the lock screen and the display-manager greeter over X11; a Wayland \
         greeter or session is not visible until it runs an X11 session. On a headless server, metrics, \
         exec, shell and power work without a display.",
        layout.exe().display(),
        creds.device_id,
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

/// `sudo rcli agent uninstall`: removes the Device from the Server, then stops and removes the service and deletes
/// the Device's local data and secrets. A Server that cannot be reached is a warning, not a failure: the local
/// install must still be removable, and an Admin can delete the Device from the Dashboard.
pub async fn uninstall(out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli agent uninstall");
    let layout = Layout { root: "/".into() };
    if let (Some(server), Some(creds)) = (recorded_server(&layout), device_credentials(&layout)) {
        if let Err(e) = rcli_agent::unregister_at(&server, &creds).await {
            writeln!(out, "warning: could not remove the Device from the Server: {e:#}")?;
        }
    }
    let secrets = crate::agent::secret_store(&layout.state_dir());
    for name in [rcli_agent::ipc::DEVICE_PASSWORD, rcli_agent::ipc::UNLOCK_PASSWORD] {
        if let Err(e) = secrets.remove(name) {
            writeln!(out, "warning: {e:#}")?;
        }
    }
    uninstall_files(&layout, &mut system)?;
    writeln!(out, "removed {UNIT}, {} and the Device's local data", layout.exe().display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            terminal_user: Some("alice".into()),
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
        assert_eq!(std::fs::metadata(&layout.state_dir()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(layout.env_file()).unwrap().permissions().mode() & 0o777, 0o600, "it holds the Server Key");
        assert_eq!(std::fs::metadata(layout.install_file()).unwrap().permissions().mode() & 0o777, 0o600);
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
            vec!["systemctl daemon-reload".to_owned(), "systemctl enable --now rcli-agent.service".to_owned()]
        );

        calls.clear();
        uninstall_files(&layout, &mut recorder(&mut calls)).unwrap();
        assert_eq!(
            calls,
            vec!["systemctl disable --now rcli-agent.service".to_owned(), "systemctl daemon-reload".to_owned()]
        );
        for gone in ["usr/local/lib/rcli/rcli", "usr/local/bin/rcli", "var/lib/rcli", "etc/systemd/system/rcli-agent.service", "etc/rcli/agent.env"] {
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

    #[test]
    fn the_recorded_terminal_user_is_the_flag_first() {
        let mut o = opts();
        assert_eq!(o.terminal_user(), "alice");
        // Without the flag, `install` falls back to SUDO_USER (the account that ran sudo), then the process account.
        o.terminal_user = None;
        let env_user = std::env::var("SUDO_USER").ok().filter(|u| !u.trim().is_empty() && u != "root");
        assert_eq!(Some(o.terminal_user()), env_user.or_else(|| Some(o.terminal_user())));
    }
}
