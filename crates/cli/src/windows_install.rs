//! `rcli agent install|uninstall` on Windows (ticket 22): `rcli.exe` in `%ProgramFiles%\rcli`, the Server Key in a
//! SYSTEM/Administrators-only config file (never on the service's command line, which `sc qc` and the registry show
//! to every user), the Device's data in `%ProgramData%\rcli`, and a SYSTEM service `rcli-agent`. The service itself
//! is hosted with the `windows-service` crate (`windows_service.rs`), so it reports SCM status instead of dying
//! with error 1053.
//!
//! Every path hangs off `Layout`, and every external command (`sc.exe`, `icacls.exe`) goes through `Run`, so the
//! layout, the service definition and the exact commands are tested on any host without touching a real machine.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::install::{Options, Run};

/// The SCM name of the service.
pub const SERVICE: &str = "rcli-agent";
/// What the Services console shows.
pub const DISPLAY_NAME: &str = "rcli Agent";
/// The named pipe the SYSTEM service accepts its session helpers on (`windows_session.rs` owns the name).
pub const PIPE: &str = rcli_agent::session::PIPE;
/// The Start Menu shortcut an install creates: running it opens the GUI window with no arguments (ticket 14).
pub const SHORTCUT: &str = "rcli.lnk";

/// Where an install puts things. Two roots, so a test can point both at a temp directory.
#[derive(Clone, Debug)]
pub struct Layout {
    pub program_files: PathBuf,
    pub program_data: PathBuf,
}

impl Layout {
    /// The real machine: `%ProgramFiles%` and `%ProgramData%` (never assumed to be on `C:`).
    #[cfg(windows)]
    pub fn real() -> Self {
        let dir = |name: &str, fallback: &str| std::env::var_os(name).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(fallback));
        Self { program_files: dir("ProgramFiles", r"C:\Program Files"), program_data: dir("ProgramData", r"C:\ProgramData") }
    }

    pub fn exe(&self) -> PathBuf {
        self.program_files.join("rcli").join("rcli.exe")
    }

    /// The Device's credentials, Device Settings and Secret Store.
    pub fn state_dir(&self) -> PathBuf {
        self.program_data.join("rcli")
    }

    /// The service config: Server Address, Server Key, Device name, state directory and helper pipe.
    pub fn config(&self) -> PathBuf {
        self.state_dir().join("service.json")
    }

    pub fn session_pipe(&self) -> &'static str {
        PIPE
    }

    /// The all-users Start Menu shortcut: visible to every account, like the service itself.
    pub fn shortcut(&self) -> PathBuf {
        self.program_data.join("Microsoft").join("Windows").join("Start Menu").join("Programs").join(SHORTCUT)
    }
}

/// The protected file the service reads at start. The Server Key lives here, not in the service's arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub server: String,
    pub key: String,
    pub name: String,
    pub state_dir: PathBuf,
    pub pipe: String,
}

impl ServiceConfig {
    pub fn new(layout: &Layout, opts: &Options) -> Self {
        Self {
            server: opts.server.clone(),
            key: opts.key.clone(),
            name: opts.name.clone(),
            state_dir: layout.state_dir(),
            pipe: layout.session_pipe().into(),
        }
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
}

/// The arguments after `rcli.exe` that start the service host; the config path is not secret, the key is in the file.
pub fn service_args(layout: &Layout) -> Vec<String> {
    vec!["agent".into(), "service".into(), "--config".into(), layout.config().display().to_string()]
}

/// The service's binary path: the installed exe, quoted if it has spaces, then its arguments.
pub fn bin_path(layout: &Layout) -> String {
    let quote = |s: String| {
        if s.contains(' ') {
            format!("\"{s}\"")
        } else {
            s
        }
    };
    let args: Vec<String> = service_args(layout).into_iter().map(quote).collect();
    format!("{} {}", quote(layout.exe().display().to_string()), args.join(" "))
}

/// The SCM definition an install creates: a SYSTEM service, started automatically at boot.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceDefinition {
    pub name: &'static str,
    pub display_name: &'static str,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub account: &'static str,
    pub start: &'static str,
}

pub fn service_definition(layout: &Layout) -> ServiceDefinition {
    ServiceDefinition {
        name: SERVICE,
        display_name: DISPLAY_NAME,
        executable: layout.exe(),
        arguments: service_args(layout),
        account: "LocalSystem",
        start: "auto",
    }
}

/// Copies the running binary to `%ProgramFiles%\rcli`, writes the protected config, records the Device Settings and
/// creates the Start Menu shortcut.
/// `exe` is the binary running the install (a download in `%TEMP%`, or the installed one on a reinstall).
pub fn install_files(layout: &Layout, opts: &Options, exe: &Path, run: Run) -> anyhow::Result<()> {
    let installed = layout.exe();
    if let Some(dir) = installed.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let same = std::fs::canonicalize(exe).ok().is_some_and(|e| std::fs::canonicalize(&installed).ok().is_some_and(|i| i == e));
    if !same {
        // A reinstall runs while the service still runs the installed exe: Windows refuses to overwrite a running
        // image but allows renaming it, so move it aside, write the new one, and put the old one back on failure.
        if installed.exists() {
            let old = installed.with_extension("old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&installed, &old)?;
            if let Err(e) = std::fs::copy(exe, &installed) {
                let _ = std::fs::rename(&old, &installed);
                return Err(e.into());
            }
        } else {
            std::fs::copy(exe, &installed)?;
        }
    }

    let state = layout.state_dir();
    std::fs::create_dir_all(&state)?;
    std::fs::write(layout.config(), ServiceConfig::new(layout, opts).to_json()?)?;
    // The config holds the Server Key and the registry copy of the binary path is world-readable: only SYSTEM and
    // Administrators may read it.
    restrict(run, &layout.config())?;

    crate::install::save_settings(&state, opts)?;
    // Everything under %ProgramData%\rcli is the Device's private data, including the DPAPI blobs.
    restrict(run, &state)?;
    create_shortcut(layout, run);
    Ok(())
}

/// The PowerShell that creates the Start Menu shortcut through WScript.Shell. Paths are single-quoted for
/// `-Command` (a `'` would end the string, so it is doubled); the shortcut starts the installed exe with no
/// arguments, which opens the GUI window.
pub fn shortcut_script(layout: &Layout) -> String {
    let quote = |path: &Path| path.display().to_string().replace('\'', "''");
    let working_dir = layout.exe().parent().map(Path::to_path_buf).unwrap_or_default();
    format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
         $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.Save()",
        quote(&layout.shortcut()),
        quote(&layout.exe()),
        quote(&working_dir),
    )
}

/// Creates the Start Menu shortcut, best effort: a locked-down Start Menu or a missing PowerShell must not fail an
/// otherwise good install.
pub fn create_shortcut(layout: &Layout, run: Run) {
    let script = shortcut_script(layout);
    let _ = run("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script]);
}

/// Restricts `path` to SYSTEM and the Administrators group, by well-known SID so a non-English Windows works too.
/// A directory passes the grant on to what is created inside it (the Device credentials and the DPAPI blobs).
pub fn restrict(run: Run, path: &Path) -> anyhow::Result<()> {
    let spec = |sid: &str| {
        if path.is_dir() {
            format!("*{sid}:(OI)(CI)F")
        } else {
            format!("*{sid}:F")
        }
    };
    let system = spec("S-1-5-18");
    let administrators = spec("S-1-5-32-544");
    run("icacls.exe", &[&path.display().to_string(), "/inheritance:r", "/grant:r", &system, &administrators])
}

/// Creates and starts the SYSTEM service; a reinstall replaces the old one (stopping it first is not an error if it
/// was not running).
pub fn install_service(layout: &Layout, run: Run) -> anyhow::Result<()> {
    // A reinstall: stop and delete whatever is there, then create it fresh so arguments and paths are current.
    let _ = run("sc.exe", &["stop", SERVICE]);
    let _ = run("sc.exe", &["delete", SERVICE]);
    let definition = service_definition(layout);
    run(
        "sc.exe",
        &[
            "create",
            definition.name,
            "binPath=",
            &bin_path(layout),
            "start=",
            definition.start,
            "DisplayName=",
            definition.display_name,
            "obj=",
            definition.account,
        ],
    )?;
    run("sc.exe", &["start", SERVICE])
}

/// Stops and deletes the service. Absent or already stopped is not an error: `sc.exe` says so and uninstall goes on.
pub fn remove_service(run: Run) {
    let _ = run("sc.exe", &["stop", SERVICE]);
    let _ = run("sc.exe", &["delete", SERVICE]);
}

/// Removes everything `install_files`/`install_service` wrote, including the Device's local data and secrets.
pub fn uninstall_files(layout: &Layout, run: Run) -> anyhow::Result<()> {
    remove_service(run);
    let _ = std::fs::remove_file(layout.shortcut());
    let state = layout.state_dir();
    if state.exists() {
        std::fs::remove_dir_all(&state)?;
    }
    let dir = layout.program_files.join("rcli");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// This Device's name, as the Device reports it: the same hostname probe as the Controller's name (S7).
#[cfg(windows)]
pub fn device_name() -> String {
    crate::config::hostname_or("windows")
}

/// `rcli agent install --server … --key …` from an elevated shell. Files, registration, then the service; a wrong
/// Server Key fails here, before anything runs as SYSTEM, and the Device ID and password are printed on success.
#[cfg(windows)]
pub async fn install(opts: Options, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::upgrade::is_root(),
        "run it from an elevated shell (Run as administrator): rcli agent install --server … --key …"
    );
    let layout = Layout::real();
    // A reinstall must not destroy a working install when the new Server Key is wrong.
    let reinstall = layout.config().exists();
    install_files(&layout, &opts, &std::env::current_exe()?, &mut crate::install::system)?;
    // Register now, so the Device ID and password exist before the service starts and a wrong key is caught here.
    let server = rcli_agent::ServerEndpoint { addr: opts.server.clone(), server_key: opts.key.clone() };
    let platform = rcli_agent::windows_platform::WindowsPlatform::new(opts.name.clone())?;
    let secrets = crate::agent::secret_store(&layout.state_dir());
    let creds = match crate::agent::load_or_register(&server, &platform, &layout.state_dir(), secrets.as_ref()).await {
        Ok(creds) => creds,
        Err(e) => {
            // Leave no half-installed Device behind; on a reinstall keep the existing one and its credentials.
            if !reinstall {
                let _ = uninstall_files(&layout, &mut crate::install::system);
            }
            return Err(e.context("cannot register this Device; is the Server Address and Server Key right?"));
        }
    };
    if let Err(e) = install_service(&layout, &mut crate::install::system) {
        if !reinstall {
            let _ = uninstall_files(&layout, &mut crate::install::system);
        }
        return Err(e.context("cannot install the rcli-agent service"));
    }
    let password = secrets.get(rcli_agent::ipc::DEVICE_PASSWORD)?.unwrap_or_default();
    writeln!(
        out,
        "installed {} and started the service {SERVICE}\nDevice ID: {}\nDevice Password: {password}\n\
         The service runs as SYSTEM and drives the screen in the active session (the lock and sign-in screens and \
         UAC prompts too).\nNext: for Ctrl+Alt+Del, allow software SAS once (see docs/usage.md), then \
         `rcli act <Device ID> key ctrl+alt+del` from a Controller.",
        layout.exe().display(),
        creds.device_id,
    )?;
    Ok(())
}

/// The Server Address and this Device's credentials, read while they still exist: `uninstall` removes the Device
/// from the Server with them before it deletes the local data.
#[cfg(windows)]
fn recorded_install(layout: &Layout) -> (Option<String>, Option<rcli_agent::Credentials>) {
    let server = ServiceConfig::load(&layout.config()).ok().map(|config| config.server);
    let creds = std::fs::read(layout.state_dir().join("credentials.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<rcli_agent::Credentials>(&bytes).ok());
    (server, creds)
}

/// `rcli agent uninstall` from an elevated shell: removes the Device from the Server, then stops and deletes the
/// service and deletes the local data and secrets. An unreachable Server is a warning, not a failure: the local
/// install must still be removable, and an Admin can delete the Device from the Dashboard.
#[cfg(windows)]
pub async fn uninstall(out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    anyhow::ensure!(crate::upgrade::is_root(), "run it from an elevated shell (Run as administrator): rcli agent uninstall");
    let layout = Layout::real();
    if let (Some(server), Some(creds)) = recorded_install(&layout) {
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
    uninstall_files(&layout, &mut crate::install::system)?;
    writeln!(out, "stopped and deleted the service {SERVICE}; removed {} and the Device's local data", layout.exe().display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcli_agent::settings::{DeviceSettings, RootAccess};

    fn temp_layout(name: &str) -> (Layout, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root =
            std::env::temp_dir().join(format!("rcli-win-install-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let layout = Layout { program_files: root.join("Program Files"), program_data: root.join("ProgramData") };
        (layout, root)
    }

    fn opts() -> Options {
        Options {
            server: "rcli.example.com:7443".into(),
            key: "rk_ab12_s3cr&t".into(),
            name: "dev-pc".into(),
            terminal_user: "alice".into(),
            allow_root: true,
        }
    }

    #[test]
    fn install_copies_the_binary_writes_the_protected_config_and_records_the_terminal_user() {
        let (layout, root) = temp_layout("files");
        let exe = root.join("download").join("rcli.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"binary").unwrap();
        let mut calls: Vec<String> = vec![];
        let mut run = |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };
        install_files(&layout, &opts(), &exe, &mut run).unwrap();

        assert_eq!(std::fs::read(layout.exe()).unwrap(), b"binary");
        let config = ServiceConfig::load(&layout.config()).unwrap();
        assert_eq!(config.server, "rcli.example.com:7443");
        assert_eq!(config.key, "rk_ab12_s3cr&t");
        assert_eq!(config.state_dir, layout.state_dir());
        assert_eq!(config.pipe, PIPE);
        let settings = DeviceSettings::load(&layout.state_dir());
        assert_eq!((settings.terminal_user.as_str(), settings.root), ("alice", RootAccess::Always));
        // SYSTEM and Administrators only, by well-known SID (a German Windows has no group called Administrators).
        assert_eq!(
            calls,
            vec![
                format!("icacls.exe {} /inheritance:r /grant:r *S-1-5-18:F *S-1-5-32-544:F", layout.config().display()),
                format!("icacls.exe {} /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F", layout.state_dir().display()),
                format!("powershell.exe -NoProfile -NonInteractive -Command {}", shortcut_script(&layout)),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_start_menu_shortcut_opens_the_installed_binary_with_no_arguments() {
        let (layout, root) = temp_layout("shortcut");
        let script = shortcut_script(&layout);
        assert!(
            layout.shortcut().ends_with(Path::new("Microsoft/Windows/Start Menu/Programs").join(SHORTCUT)),
            "{}",
            layout.shortcut().display()
        );
        assert!(script.contains(&format!("TargetPath='{}'", layout.exe().display())), "{script}");
        assert!(script.contains(&layout.shortcut().display().to_string()), "{script}");
        assert!(!script.contains(" agent "), "the shortcut must open the window, not a subcommand: {script}");

        // A Start Menu that cannot be written (locked down, no PowerShell) is not an install failure.
        let download = root.join("download").join("rcli.exe");
        std::fs::create_dir_all(download.parent().unwrap()).unwrap();
        std::fs::write(&download, b"binary").unwrap();
        let failed = install_files(&layout, &opts(), &download, &mut |program, _args| {
            anyhow::ensure!(program != "powershell.exe", "powershell is unavailable");
            Ok(())
        });
        failed.unwrap();
        assert!(layout.exe().exists(), "the rest of the install still happened");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_service_is_system_started_at_boot_and_never_gets_the_server_key_on_its_command_line() {
        let (layout, root) = temp_layout("service");
        let definition = service_definition(&layout);
        assert_eq!(definition.name, "rcli-agent");
        assert_eq!(definition.account, "LocalSystem");
        assert_eq!(definition.start, "auto");
        assert_eq!(definition.executable, layout.program_files.join("rcli").join("rcli.exe"));
        assert_eq!(
            definition.arguments,
            vec!["agent".to_owned(), "service".to_owned(), "--config".to_owned(), layout.config().display().to_string()]
        );
        let bin = bin_path(&layout);
        assert!(bin.contains(&format!("\"{}\"", layout.exe().display())), "a path with spaces must be quoted: {bin}");
        assert!(!bin.contains("rk_"), "the Server Key must not be visible in `sc qc`: {bin}");

        let mut calls: Vec<String> = vec![];
        let mut run = |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };
        install_service(&layout, &mut run).unwrap();
        assert_eq!(
            calls,
            vec![
                "sc.exe stop rcli-agent".to_owned(),
                "sc.exe delete rcli-agent".to_owned(),
                format!("sc.exe create rcli-agent binPath= {} start= auto DisplayName= rcli Agent obj= LocalSystem", bin_path(&layout)),
                "sc.exe start rcli-agent".to_owned(),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_stops_the_service_and_removes_the_binary_the_data_and_the_secrets() {
        let (layout, root) = temp_layout("uninstall");
        let exe = root.join("rcli.exe");
        std::fs::write(&exe, b"binary").unwrap();
        let mut calls: Vec<String> = vec![];
        {
            let mut run = |program: &str, args: &[&str]| {
                calls.push(format!("{program} {}", args.join(" ")));
                Ok(())
            };
            install_files(&layout, &opts(), &exe, &mut run).unwrap();
        }
        std::fs::create_dir_all(layout.state_dir().join("secrets")).unwrap();
        std::fs::write(layout.state_dir().join("secrets").join("device_password"), b"sealed").unwrap();
        std::fs::create_dir_all(layout.shortcut().parent().unwrap()).unwrap();
        std::fs::write(layout.shortcut(), b"shortcut").unwrap();
        assert!(layout.exe().exists() && layout.state_dir().exists() && layout.shortcut().exists());

        calls.clear();
        {
            let mut run = |program: &str, args: &[&str]| {
                calls.push(format!("{program} {}", args.join(" ")));
                Ok(())
            };
            uninstall_files(&layout, &mut run).unwrap();
        }
        assert_eq!(calls, vec!["sc.exe stop rcli-agent".to_owned(), "sc.exe delete rcli-agent".to_owned()]);
        assert!(!layout.state_dir().exists() && !layout.program_files.join("rcli").exists(), "an install file is left behind");
        assert!(!layout.shortcut().exists(), "the Start Menu shortcut is left behind");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_service_config_round_trips_through_its_file() {
        let (layout, root) = temp_layout("config");
        std::fs::create_dir_all(layout.state_dir()).unwrap();
        let config = ServiceConfig::new(&layout, &opts());
        std::fs::write(layout.config(), config.to_json().unwrap()).unwrap();
        assert_eq!(ServiceConfig::load(&layout.config()).unwrap(), config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_reinstall_moves_the_running_binary_aside_before_writing_the_new_one() {
        let (layout, root) = temp_layout("replace");
        std::fs::create_dir_all(layout.exe().parent().unwrap()).unwrap();
        std::fs::write(layout.exe(), b"running build").unwrap();
        let download = root.join("download").join("rcli.exe");
        std::fs::create_dir_all(download.parent().unwrap()).unwrap();
        std::fs::write(&download, b"new build").unwrap();
        let mut run = |_program: &str, _args: &[&str]| Ok(());
        install_files(&layout, &opts(), &download, &mut run).unwrap();
        assert_eq!(std::fs::read(layout.exe()).unwrap(), b"new build");
        assert_eq!(std::fs::read(layout.exe().with_extension("old.exe")).unwrap(), b"running build");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_reinstall_does_not_copy_the_installed_binary_onto_itself() {
        let (layout, root) = temp_layout("reinstall");
        std::fs::create_dir_all(layout.exe().parent().unwrap()).unwrap();
        std::fs::write(layout.exe(), b"already installed").unwrap();
        let mut calls: Vec<String> = vec![];
        let mut run = |program: &str, args: &[&str]| {
            calls.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };
        install_files(&layout, &opts(), &layout.exe(), &mut run).unwrap();
        assert_eq!(std::fs::read(layout.exe()).unwrap(), b"already installed");
        let _ = std::fs::remove_dir_all(root);
    }
}
