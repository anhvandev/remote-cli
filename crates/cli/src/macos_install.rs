//! `rcli agent install|uninstall` on macOS (ticket 24): the app bundle in /Applications (TCC grants stick to it),
//! a root LaunchDaemon running the Agent, and a LaunchAgent in the LoginWindow and Aqua sessions doing its screen
//! work (ADR 0001). Every path hangs off `Layout::root`, and every external command goes through `run`, so the
//! file layout and the `launchctl` calls are tested without touching the Device.

use std::path::{Path, PathBuf};

use crate::install::{system, write};
pub use crate::install::{Options, Run};

// shortcut: ADR 0001 calls the root process "a root daemon" and this job is a launchd LaunchDaemon, so the label
// and the LaunchDaemon wording stay; prose elsewhere says "Agent service" (GLOSSARY.md: Agent — _Avoid_: daemon).
pub const DAEMON: &str = "dev.remotecli.agent";
pub const SESSION: &str = "dev.remotecli.agent.session";
const INFO_PLIST: &str = include_str!("../../../packaging/macos/Info.plist");

pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    fn at(&self, path: &str) -> PathBuf {
        crate::install::at(&self.root, path)
    }
    pub fn app(&self) -> PathBuf {
        self.at("/Applications/rcli.app")
    }
    pub fn exe(&self) -> PathBuf {
        self.app().join("Contents/MacOS/rcli")
    }
    /// `rcli` on the PATH for the terminal.
    pub fn link(&self) -> PathBuf {
        self.at("/usr/local/bin/rcli")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.at("/Library/Application Support/rcli")
    }
    pub fn daemon_plist(&self) -> PathBuf {
        self.at(&format!("/Library/LaunchDaemons/{DAEMON}.plist"))
    }
    pub fn session_plist(&self) -> PathBuf {
        self.at(&format!("/Library/LaunchAgents/{SESSION}.plist"))
    }
    /// Where the Agent service listens for the session helpers, beside the IPC socket.
    pub fn session_socket(&self) -> PathBuf {
        self.at("/var/run/rcli/session.sock")
    }
    fn log(&self) -> PathBuf {
        self.at("/Library/Logs/rcli-agent.log")
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn strings(items: &[&str]) -> String {
    items.iter().map(|s| format!("    <string>{}</string>\n", xml(s))).collect()
}

fn plist(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{body}</dict>\n</plist>\n"
    )
}

/// The root LaunchDaemon: the Agent itself. The Server Key travels in its environment, never on a command line
/// (`ps` shows every process's arguments to every user), so this file is root-only.
pub fn daemon_plist(layout: &Layout, opts: &Options) -> String {
    let exe = layout.exe().display().to_string();
    let state = layout.state_dir().display().to_string();
    let session = layout.session_socket().display().to_string();
    let log = layout.log().display().to_string();
    let args = [&exe, "agent", "run", "--server", &opts.server, "--state-dir", &state, "--name", &opts.name, "--session-socket", &session];
    plist(&format!(
        "  <key>Label</key><string>{DAEMON}</string>\n  <key>ProgramArguments</key>\n  <array>\n{}  </array>\n  \
         <key>EnvironmentVariables</key>\n  <dict>\n    <key>RCLI_SERVER_KEY</key><string>{}</string>\n  </dict>\n  \
         <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key><true/>\n  <key>StandardOutPath</key><string>{log}</string>\n  \
         <key>StandardErrorPath</key><string>{log}</string>\n",
        strings(&args),
        xml(&opts.key),
    ))
}

/// The session helper: launchd starts one in the login window (as root) and one in each user's Aqua session.
pub fn session_plist(layout: &Layout) -> String {
    let exe = layout.exe().display().to_string();
    let session = layout.session_socket().display().to_string();
    plist(&format!(
        "  <key>Label</key><string>{SESSION}</string>\n  <key>LimitLoadToSessionType</key>\n  <array>\n{}  </array>\n  \
         <key>ProgramArguments</key>\n  <array>\n{}  </array>\n  <key>RunAtLoad</key><true/>\n  \
         <key>KeepAlive</key><true/>\n  <key>ProcessType</key><string>Interactive</string>\n",
        strings(&["LoginWindow", "Aqua"]),
        strings(&[&exe, "agent", "session", "--socket", &session]),
    ))
}

/// The app bundle's Info.plist, shared with `packaging/macos/build-dmg.sh`.
pub fn info_plist() -> String {
    INFO_PLIST.replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
}

/// The `.app` that contains `exe`, if it runs from one (the `.dmg` or an unpacked copy).
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_owned())
}

/// Puts the app in /Applications and records the Device Settings the install asked for. `exe` is the running binary.
pub fn install_files(layout: &Layout, opts: &Options, exe: &Path, run: Run) -> anyhow::Result<()> {
    let app = layout.app();
    match bundle_of(exe) {
        Some(bundle) if bundle == app => {}
        Some(bundle) => {
            let _ = std::fs::remove_dir_all(&app);
            run("/usr/bin/ditto", &[&bundle.to_string_lossy(), &app.to_string_lossy()])?;
        }
        // The curl one-liner downloads a bare binary: build the same bundle around it.
        None => {
            let _ = std::fs::remove_dir_all(&app);
            std::fs::create_dir_all(layout.exe().parent().unwrap())?;
            std::fs::copy(exe, layout.exe())?;
            write(&app.join("Contents/Info.plist"), &info_plist(), 0o644)?;
            // Ad-hoc sign the bundle so TCC files the grants under the app's identifier.
            run("/usr/bin/codesign", &["--force", "--sign", "-", &app.to_string_lossy()])?;
        }
    }
    let link = layout.link();
    std::fs::create_dir_all(link.parent().unwrap())?;
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(layout.exe(), &link)?;

    let state = layout.state_dir();
    std::fs::create_dir_all(&state)?;
    std::fs::set_permissions(&state, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    crate::install::save_settings(&state, opts)?;
    // No secret here: `uninstall` needs the Server Address to remove this Device, and the Server Key is in the
    // root-only LaunchDaemon plist.
    write(&state.join("install.json"), &serde_json::json!({ "server": opts.server }).to_string(), 0o600)
}

/// Writes both launchd jobs and starts them: the Agent's LaunchDaemon now, the session helper in the console user's session now
/// (and launchd starts it in every later login window and Aqua session).
pub fn install_jobs(layout: &Layout, opts: &Options, console_uid: Option<u32>, run: Run) -> anyhow::Result<()> {
    write(&layout.daemon_plist(), &daemon_plist(layout, opts), 0o600)?;
    write(&layout.session_plist(), &session_plist(layout), 0o644)?;
    // A reinstall replaces the running jobs; on a first install there is nothing to stop.
    let _ = run("/bin/launchctl", &["bootout", &format!("system/{DAEMON}")]);
    run("/bin/launchctl", &["bootstrap", "system", &layout.daemon_plist().to_string_lossy()])?;
    if let Some(uid) = console_uid.filter(|&uid| uid != 0) {
        let _ = run("/bin/launchctl", &["bootout", &format!("gui/{uid}/{SESSION}")]);
        run("/bin/launchctl", &["bootstrap", &format!("gui/{uid}"), &layout.session_plist().to_string_lossy()])?;
    }
    Ok(())
}

/// Stops both jobs and deletes everything `install_files`/`install_jobs` wrote, including the Device's local data.
pub fn uninstall_files(layout: &Layout, console_uid: Option<u32>, run: Run) -> anyhow::Result<()> {
    let _ = run("/bin/launchctl", &["bootout", &format!("system/{DAEMON}")]);
    if let Some(uid) = console_uid.filter(|&uid| uid != 0) {
        let _ = run("/bin/launchctl", &["bootout", &format!("gui/{uid}/{SESSION}")]);
    }
    for file in [layout.daemon_plist(), layout.session_plist(), layout.link(), layout.session_socket()] {
        let _ = std::fs::remove_file(file);
    }
    for dir in [layout.state_dir(), layout.app()] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

/// The user logged in on the console (root at the login window).
fn console_uid() -> Option<u32> {
    Some(std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/dev/console").ok()?))
}

/// `sudo rcli agent install`: app, settings, registration, then both launchd jobs.
pub async fn install(opts: Options, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli agent install --server … --key …");
    let layout = Layout { root: "/".into() };
    install_files(&layout, &opts, &std::env::current_exe()?, &mut system)?;
    // Register now, so a wrong Server Key fails here and the Device ID can be shown; the service finds the credentials.
    let server = rcli_agent::ServerEndpoint { addr: opts.server.clone(), server_key: opts.key.clone() };
    let platform = rcli_agent::macos::MacPlatform::new(opts.name.clone())?;
    let secrets = crate::agent::secret_store(&layout.state_dir());
    let creds = crate::agent::load_or_register(&server, &platform, &layout.state_dir(), secrets.as_ref()).await?;
    install_jobs(&layout, &opts, console_uid(), &mut system)?;
    let password = secrets.get(rcli_agent::ipc::DEVICE_PASSWORD)?.unwrap_or_default();
    writeln!(
        out,
        "installed {} with the launchd jobs {DAEMON} and {SESSION}\nDevice ID: {}\nDevice Password: {password}\n\
         Next: open rcli from Applications and allow it Screen Recording and Accessibility \
         (System Settings > Privacy & Security), so Controllers can see and use the screen.",
        layout.app().display(),
        creds.device_id,
    )?;
    Ok(())
}

/// `sudo rcli agent uninstall`: removes the Device from the Server, then stops and removes both jobs, the app and
/// the Device's local data and secrets. An unreachable Server is a warning, not a failure: the local install must
/// still go away, and an Admin can delete the Device from the Dashboard.
pub async fn uninstall(out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it as root: sudo rcli agent uninstall");
    let layout = Layout { root: "/".into() };
    let server = std::fs::read_to_string(layout.state_dir().join("install.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v["server"].as_str().map(str::to_owned));
    let creds = std::fs::read(layout.state_dir().join("credentials.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<rcli_agent::Credentials>(&bytes).ok());
    if let (Some(server), Some(creds)) = (server, creds) {
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
    uninstall_files(&layout, console_uid(), &mut system)?;
    writeln!(out, "removed the launchd jobs {DAEMON} and {SESSION}, {} and the Device's local data", layout.app().display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcli_agent::settings::{DeviceSettings, RootAccess};

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcli-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn opts() -> Options {
        Options {
            server: "rcli.example.com:7443".into(),
            key: "rk_ab12_s3cr&t".into(),
            name: "mac-mini".into(),
            terminal_user: "alice".into(),
            allow_root: true,
        }
    }

    #[test]
    fn install_puts_the_app_both_jobs_and_the_settings_in_place_and_uninstall_removes_them() {
        let root = temp_root("roundtrip");
        let layout = Layout { root: root.clone() };
        let exe = root.join("download/rcli");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"binary").unwrap();
        let mut calls: Vec<String> = vec![];
        let mut run = |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };

        install_files(&layout, &opts(), &exe, &mut run).unwrap();
        install_jobs(&layout, &opts(), Some(501), &mut run).unwrap();

        let app = root.join("Applications/rcli.app");
        assert_eq!(std::fs::read(app.join("Contents/MacOS/rcli")).unwrap(), b"binary");
        let info = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(info.contains("<string>dev.remotecli.rcli</string>") && !info.contains("{{VERSION}}"));
        assert_eq!(std::fs::read_link(root.join("usr/local/bin/rcli")).unwrap(), app.join("Contents/MacOS/rcli"));
        let settings = DeviceSettings::load(&root.join("Library/Application Support/rcli"));
        let install = std::fs::read_to_string(root.join("Library/Application Support/rcli/install.json")).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&install).unwrap()["server"], "rcli.example.com:7443");
        assert_eq!((settings.terminal_user.as_str(), settings.root), ("alice", RootAccess::Always));

        use std::os::unix::fs::PermissionsExt;
        let daemon = root.join("Library/LaunchDaemons/dev.remotecli.agent.plist");
        let session = root.join("Library/LaunchAgents/dev.remotecli.agent.session.plist");
        assert_eq!(std::fs::metadata(&daemon).unwrap().permissions().mode() & 0o777, 0o600, "it holds the Server Key");
        assert_eq!(std::fs::metadata(&session).unwrap().permissions().mode() & 0o777, 0o644);

        let app = app.display();
        let daemon_s = daemon.display();
        let session_s = session.display();
        assert_eq!(
            calls,
            vec![
                format!("/usr/bin/codesign --force --sign - {app}"),
                "/bin/launchctl bootout system/dev.remotecli.agent".to_owned(),
                format!("/bin/launchctl bootstrap system {daemon_s}"),
                "/bin/launchctl bootout gui/501/dev.remotecli.agent.session".to_owned(),
                format!("/bin/launchctl bootstrap gui/501 {session_s}"),
            ]
        );

        calls.clear();
        uninstall_files(&layout, Some(501), &mut |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls,
            vec!["/bin/launchctl bootout system/dev.remotecli.agent", "/bin/launchctl bootout gui/501/dev.remotecli.agent.session"]
        );
        for gone in ["Applications/rcli.app", "usr/local/bin/rcli", "Library/Application Support/rcli"] {
            assert!(!root.join(gone).exists(), "{gone} is left behind");
        }
        assert!(!daemon.exists() && !session.exists(), "a launchd job is left behind");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_launchdaemon_runs_the_agent_as_root_with_the_key_out_of_its_arguments() {
        let layout = Layout { root: "/".into() };
        let daemon = daemon_plist(&layout, &opts());
        let expected_args = "    <string>/Applications/rcli.app/Contents/MacOS/rcli</string>\n    <string>agent</string>\n    \
             <string>run</string>\n    <string>--server</string>\n    <string>rcli.example.com:7443</string>\n    \
             <string>--state-dir</string>\n    <string>/Library/Application Support/rcli</string>\n    \
             <string>--name</string>\n    <string>mac-mini</string>\n    <string>--session-socket</string>\n    \
             <string>/var/run/rcli/session.sock</string>\n";
        assert!(daemon.contains(expected_args), "{daemon}");
        assert!(daemon.contains("<key>RCLI_SERVER_KEY</key><string>rk_ab12_s3cr&amp;t</string>"), "{daemon}");
        assert!(daemon.contains("<key>KeepAlive</key><true/>"));

        let session = session_plist(&layout);
        assert!(session.contains(
            "<key>LimitLoadToSessionType</key>\n  <array>\n    <string>LoginWindow</string>\n    <string>Aqua</string>\n  </array>"
        ));
        assert!(session.contains("<string>session</string>\n    <string>--socket</string>\n    <string>/var/run/rcli/session.sock</string>"));
        assert!(!session.contains("rk_"), "the session helper never gets the Server Key");
    }

    #[test]
    fn plists_are_valid_property_lists() {
        let layout = Layout { root: "/".into() };
        let dir = temp_root("plutil");
        for (name, text) in [("daemon", daemon_plist(&layout, &opts())), ("session", session_plist(&layout)), ("info", info_plist())] {
            let path = dir.join(format!("{name}.plist"));
            std::fs::write(&path, text).unwrap();
            let lint = std::process::Command::new("/usr/bin/plutil").arg("-lint").arg(&path).output().unwrap();
            assert!(lint.status.success(), "{name}: {}", String::from_utf8_lossy(&lint.stdout));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
