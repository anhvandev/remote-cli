//! The Agent GUI window (ticket 14): a small egui window that talks to the Agent over its local IPC.
//! Built only with `--features gui`; the default `rcli` binary links no display libraries.

use std::path::PathBuf;

use eframe::egui;
use rcli_agent::{
    ipc::{self, Request},
    settings::{AcceptMode, Change, DeviceSettings, RootAccess},
};
use serde_json::{json, Value};

/// Opens the GUI window. `socket` overrides the Agent's IPC socket (`RCLI_AGENT_SOCKET`).
pub fn run(socket: Option<PathBuf>) -> anyhow::Result<()> {
    let socket = socket.unwrap_or_else(ipc::default_socket);
    eframe::run_native("rcli", eframe::NativeOptions::default(), Box::new(move |_| Ok(Box::new(App::new(socket)))))
        .map_err(|e| anyhow::anyhow!("cannot open the GUI: {e}"))?;
    Ok(())
}

struct App {
    socket: PathBuf,
    rt: tokio::runtime::Runtime,
    /// The Agent's `info` reply: `device_id`, `connected`, `server`, `controllers`, `settings`.
    info: Value,
    /// The prompts waiting for the person at the Device (tickets 07/08).
    prompts: Value,
    /// The new Device Password being typed.
    password: String,
    /// The Device Password after a reveal (root), for the Copy button.
    shown: Option<String>,
    /// The Server Key and whether an unlock password is set, after an elevated reveal.
    secrets: Option<Value>,
    /// The Device Settings as the Agent last reported them (root-only), once the person loaded them. `None` until
    /// then, so the editors never start from made-up defaults.
    loaded: Option<DeviceSettings>,
    /// The Device Settings editor: seeded from `loaded`, never sent as a whole (only changed fields are).
    edited: DeviceSettings,
    /// The new unlock password being typed.
    unlock: String,
    /// The last action's message.
    note: String,
    /// The install form (macOS and Windows, shown while no Agent answers): Server Address and Server Key.
    #[cfg(any(target_os = "macos", windows))]
    install_server: String,
    #[cfg(any(target_os = "macos", windows))]
    install_key: String,
}

impl App {
    fn new(socket: PathBuf) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let info = fetch(&rt, &socket);
        Self {
            socket,
            rt,
            info,
            prompts: json!([]),
            password: String::new(),
            shown: None,
            secrets: None,
            loaded: None,
            edited: DeviceSettings::default(),
            unlock: String::new(),
            note: String::new(),
            #[cfg(any(target_os = "macos", windows))]
            install_server: String::new(),
            #[cfg(any(target_os = "macos", windows))]
            install_key: String::new(),
        }
    }

    /// One IPC round-trip; an `error` reply comes back as `{"error": ...}`.
    fn ipc(&self, request: &Request) -> Value {
        match self.rt.block_on(ipc::send(&self.socket, request)) {
            Ok(reply) => reply,
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// One IPC request, run elevated when the Agent refuses only for lack of root.
    fn ipc_elevated(&self, request: &Request, args: &[&str], stdin: Option<&str>) -> Value {
        let reply = self.ipc(request);
        if reply.get("error").is_none() || !refused_for_root(&reply) {
            return reply;
        }
        match elevated(args, stdin) {
            // Most commands print one human line; `info --json` prints the reply as JSON.
            Ok(out) => serde_json::from_str(&out).unwrap_or_else(|_| json!({ "message": out })),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    fn reveal(&mut self) {
        let reply = self.ipc(&Request::PasswordShow);
        self.shown = match reply["password"].as_str() {
            Some(p) => Some(p.to_owned()),
            // Refused only for lack of root: ask the OS to run the read elevated (polkit, macOS authorization).
            None if refused_for_root(&reply) => match elevated(&["agent", "password", "show"], None) {
                Ok(p) => Some(p),
                Err(e) => {
                    self.note = format!("cannot show the Device Password: {e}");
                    None
                }
            },
            None => {
                self.note = format!("cannot show the Device Password: {}", reply["error"].as_str().unwrap_or_default());
                None
            }
        };
    }

    fn change(&mut self) {
        let reply = self.ipc(&Request::PasswordSet { password: self.password.clone() });
        self.note = if let Some(message) = reply["message"].as_str() {
            message.to_owned()
        } else if refused_for_root(&reply) {
            match elevated(&["agent", "password", "set"], Some(&self.password)) {
                Ok(_) => "the Device Password was changed".to_owned(),
                Err(e) => format!("cannot change the Device Password: {e}"),
            }
        } else {
            format!("cannot change the Device Password: {}", reply["error"].as_str().unwrap_or_default())
        };
        self.password.clear();
        self.shown = None;
    }

    /// Applies only the Device Settings fields the person changed (root; the Agent saves and applies them at once).
    /// Each field goes on its own and every failure is reported: one refused change must not hide another one.
    fn set_settings(&mut self) {
        let Some(loaded) = self.loaded.clone() else {
            self.note = "load the Device Settings first: they are root-only".into();
            return;
        };
        let change = Change::between(&loaded, &self.edited);
        if change.is_empty() {
            self.note = "no Device Settings changed".into();
            return;
        }
        let mut lines = Vec::new();
        let mut applied = Change::default();
        if let Some(user) = change.terminal_user.clone() {
            let request = Request::TerminalUserSet { user: user.clone() };
            match self.send_setting("Terminal User", &request, &["agent", "terminal-user", "set", &user]) {
                Ok(message) => {
                    applied.terminal_user = Some(user);
                    lines.push(message);
                }
                Err(e) => lines.push(e),
            }
        }
        if let Some(root) = change.root {
            let request = Request::RootSet { root };
            match self.send_setting("root access", &request, &["agent", "root", root.name()]) {
                Ok(message) => {
                    applied.root = Some(root);
                    lines.push(message);
                }
                Err(e) => lines.push(e),
            }
        }
        if let Some(accept) = change.accept {
            let request = Request::AcceptSet { accept };
            match self.send_setting("accept mode", &request, &["agent", "accept", accept.name()]) {
                Ok(message) => {
                    applied.accept = Some(accept);
                    lines.push(message);
                }
                Err(e) => lines.push(e),
            }
        }
        if let Some(loaded) = self.loaded.as_mut() {
            applied.apply(loaded);
        }
        self.note = lines.join("\n");
    }

    /// One changed setting, run elevated when the Agent refuses only for lack of root; `Err` names the setting.
    fn send_setting(&self, field: &str, request: &Request, args: &[&str]) -> Result<String, String> {
        let reply = self.ipc_elevated(request, args, None);
        match reply["message"].as_str() {
            Some(message) => Ok(message.to_owned()),
            None => Err(format!("{field}: {}", reply["error"].as_str().unwrap_or("the Agent did not report a result"))),
        }
    }

    /// The `info` reply as root sees it: the ordinary IPC when this process is already root, else one OS
    /// authorization prompt (`rcli agent info --json`, like Reveal). The Settings and the secrets are root-only, so
    /// the unelevated reply never carries them.
    fn root_info(&self) -> Result<Value, String> {
        let reply = self.ipc(&Request::Info);
        if reply.get("settings").is_some() || reply.get("server_key").is_some() {
            return Ok(reply);
        }
        let out = elevated(&["agent", "info", "--json"], None).map_err(|e| e.to_string())?;
        serde_json::from_str(&out).map_err(|e| format!("the elevated `rcli agent info` did not return JSON: {e}"))
    }

    /// Reads the Device Settings through the elevated path and fills the editors from that reply. Until this
    /// succeeds the editors are disabled: a click on Apply must never send made-up values.
    fn load_settings(&mut self) {
        match self.root_info() {
            Ok(reply) => match DeviceSettings::from_info(&reply) {
                Ok(settings) => {
                    self.edited = settings.clone();
                    self.loaded = Some(settings);
                    self.note = "the Device Settings were loaded".into();
                }
                Err(e) => self.note = format!("cannot edit the Device Settings: {e}"),
            },
            Err(e) => self.note = format!("cannot read the Device Settings: {e}"),
        }
    }

    /// Stores a new unlock password (root; over stdin, never on a command line).
    fn set_unlock(&mut self) {
        let request = Request::UnlockPasswordSet { password: self.unlock.clone() };
        let reply = self.ipc_elevated(&request, &["agent", "unlock-password", "set"], Some(&self.unlock));
        self.note = note(&reply);
        self.unlock.clear();
    }

    fn clear_unlock(&mut self) {
        let reply = self.ipc_elevated(&Request::UnlockPasswordClear, &["agent", "unlock-password", "clear"], None);
        self.note = note(&reply);
    }

    /// The Server Key and whether an unlock password is set: root-only fields of `info`, so this asks the OS when not
    /// root.
    fn reveal_secrets(&mut self) {
        match self.root_info() {
            Ok(reply) if reply.get("server_key").is_some() => self.secrets = Some(reply),
            Ok(reply) => {
                self.note = format!("cannot read the Server Key: {}", reply["error"].as_str().unwrap_or("the Agent did not include it"))
            }
            Err(e) => self.note = format!("cannot read the Server Key: {e}"),
        }
    }

    /// Installs the Agent service from this app after the OS elevation prompt (a macOS authorization dialog, a
    /// Windows UAC prompt). The person running the app becomes the Terminal User. On macOS permission grants belong
    /// to the installed app, so it is reopened; on Windows the downloaded exe may be deleted once the service runs.
    #[cfg(any(target_os = "macos", windows))]
    fn install(&mut self) {
        let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        let mut args = vec!["agent", "install", "--server", self.install_server.trim(), "--key", self.install_key.trim()];
        if !user.is_empty() {
            args.extend(["--terminal-user", &user]);
        }
        match elevated(&args, None) {
            Ok(out) => {
                self.note = out;
                #[cfg(target_os = "macos")]
                {
                    let installed = std::path::Path::new("/Applications/rcli.app");
                    if !std::env::current_exe().is_ok_and(|exe| exe.starts_with(installed)) {
                        let _ = std::process::Command::new("open").arg("-n").arg(installed).spawn();
                        std::process::exit(0);
                    }
                }
            }
            Err(e) => self.note = format!("install failed: {e}"),
        }
    }

    /// Grants root to one Controller for this session only (root).
    fn grant_root(&mut self, who: &str) {
        let reply = self.ipc_elevated(&Request::RootGrant { who: who.to_owned() }, &["agent", "root", "grant", who], None);
        self.note = note(&reply);
    }

    /// The person at the Device's answer to a waiting prompt. Accepting needs root (the OS prompt);
    /// refusing is open to anyone and goes straight over the IPC.
    fn answer_prompt(&mut self, id: u64, accept: bool) {
        let request = Request::PromptAnswer { id, accept };
        let reply = if accept {
            let id = id.to_string();
            self.ipc_elevated(&request, &["agent", "prompt", "answer", &id, "accept"], None)
        } else {
            self.ipc(&request)
        };
        self.note = note(&reply);
    }
}

/// The message to show for an IPC or elevated reply.
fn note(reply: &Value) -> String {
    match reply["message"].as_str() {
        Some(message) => message.to_owned(),
        None => format!("error: {}", reply["error"].as_str().unwrap_or_default()),
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.info = fetch(&self.rt, &self.socket);
        self.prompts = self.ipc(&Request::Prompts);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rcli");
            for line in self.note.lines() {
                ui.label(line);
            }
            if let Some(error) = self.info["error"].as_str() {
                ui.label(format!("Agent unreachable: {error}"));
                #[cfg(any(target_os = "macos", windows))]
                {
                    ui.separator();
                    ui.label("Install the Agent on this Device (the Dashboard's Connect page shows both values)");
                    ui.horizontal(|ui| {
                        ui.label("Server Address:");
                        ui.text_edit_singleline(&mut self.install_server);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Server Key:");
                        ui.text_edit_singleline(&mut self.install_key);
                    });
                    if ui.button("Install").clicked() {
                        self.install();
                    }
                }
            } else {
                ui.label(format!("Device ID: {}", self.info["device_id"].as_str().unwrap_or("-")));
                ui.label(format!("Connection: {}", if self.info["connected"] == true { "connected" } else { "disconnected" }));
                ui.label(format!("Server: {}", self.info["server"].as_str().unwrap_or("-")));
                if ui.button("Reveal Server Key").clicked() {
                    self.reveal_secrets();
                }
                if let Some(secrets) = &self.secrets {
                    ui.label(format!("Server Key: {}", secrets["server_key"].as_str().unwrap_or("-")));
                }
                ui.separator();
                ui.label("Device Settings");
                match self.loaded.clone() {
                    Some(settings) => {
                        ui.label(format!("Terminal user: {}", settings.terminal_user));
                        ui.label(format!("Root: {}", settings.root.name()));
                        ui.label(format!("Accept: {}", settings.accept.name()));
                        ui.horizontal(|ui| {
                            ui.label("Terminal user:");
                            ui.text_edit_singleline(&mut self.edited.terminal_user);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Root:");
                            for level in [RootAccess::Never, RootAccess::Ask, RootAccess::Always] {
                                ui.selectable_value(&mut self.edited.root, level, level.name());
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Accept:");
                            for mode in [AcceptMode::Password, AcceptMode::PasswordAccept] {
                                ui.selectable_value(&mut self.edited.accept, mode, mode.name());
                            }
                        });
                        // Only the fields the person changed are sent (see `Change::between`).
                        if ui.button("Apply Device Settings").clicked() {
                            self.set_settings();
                        }
                        if ui.button("Reload Device Settings").clicked() {
                            self.load_settings();
                        }
                    }
                    None => {
                        // The Agent hides Settings from an unelevated `info` reply, so they are read through the same
                        // OS authorization prompt as Reveal. Apply stays unavailable until that reply fills the editors.
                        ui.label("Root only: loading asks for permission the way Reveal does (polkit, UAC or macOS authorization).");
                        if ui.button("Load Device Settings").clicked() {
                            self.load_settings();
                        }
                    }
                }
                ui.separator();
                ui.label("Device Password");
                ui.horizontal(|ui| {
                    if ui.button("Reveal").clicked() {
                        self.reveal();
                    }
                    if ui.add_enabled(self.shown.is_some(), egui::Button::new("Copy")).clicked() {
                        if let Some(p) = self.shown.clone() {
                            ctx.copy_text(p);
                            self.note = "the Device Password was copied".into();
                        }
                    }
                });
                if let Some(shown) = &self.shown {
                    ui.label(format!("Device Password: {shown}"));
                }
                ui.horizontal(|ui| {
                    ui.label("New password:");
                    ui.text_edit_singleline(&mut self.password);
                    if ui.button("Change").clicked() {
                        self.change();
                    }
                });
                ui.separator();
                ui.label("Unlock password");
                if let Some(secrets) = &self.secrets {
                    ui.label(if secrets["unlock_password_set"] == true { "an unlock password is set" } else { "no unlock password is set" });
                }
                ui.horizontal(|ui| {
                    ui.label("New:");
                    ui.text_edit_singleline(&mut self.unlock);
                    if ui.button("Set").clicked() {
                        self.set_unlock();
                    }
                    if ui.button("Clear").clicked() {
                        self.clear_unlock();
                    }
                });
                ui.separator();
                ui.label("Waiting for you");
                let waiting: Vec<Value> = self.prompts.as_array().cloned().unwrap_or_default();
                if waiting.is_empty() {
                    ui.label("nothing is waiting");
                }
                for p in waiting {
                    let id = p["id"].as_u64().unwrap_or_default();
                    let who = p["who"].as_str().unwrap_or_default();
                    let what = match p["kind"].as_str() {
                        Some("root") => format!("{who} asks for root access"),
                        _ => format!("{who} wants to connect"),
                    };
                    ui.horizontal(|ui| {
                        ui.label(what);
                        if ui.button("Accept").clicked() {
                            self.answer_prompt(id, true);
                        }
                        if ui.button("Reject").clicked() {
                            self.answer_prompt(id, false);
                        }
                    });
                }
                ui.separator();
                ui.label("Being controlled by");
                let controllers = self.info["controllers"].as_array().cloned().unwrap_or_default();
                if controllers.is_empty() {
                    ui.label("no Controller is connected");
                }
                for controller in &controllers {
                    let who = controller.as_str().unwrap_or_default().to_owned();
                    ui.horizontal(|ui| {
                        ui.label(&who);
                        if ui.button("Disconnect").clicked() {
                            let reply = self.ipc(&Request::Disconnect { who: who.clone() });
                            self.note = reply["message"].as_str().unwrap_or_default().to_owned();
                        }
                        if ui.button("Grant root to this session").clicked() {
                            self.grant_root(&who);
                        }
                    });
                }
            }
            #[cfg(target_os = "macos")]
            {
                ui.separator();
                ui.label("macOS permissions (Live View and remote input need them, also at the lock and login screens)");
                let (screen, ax) = rcli_agent::macos::grants();
                let mark = |granted: bool| if granted { "allowed" } else { "not allowed yet" };
                ui.label(format!("Screen Recording: {}", mark(screen)));
                ui.label(format!("Accessibility: {}", mark(ax)));
                if !(screen && ax) {
                    ui.label("Click Allow, switch rcli on in each list System Settings opens, then reopen rcli.");
                    ui.horizontal(|ui| {
                        if ui.button("Allow").clicked() {
                            rcli_agent::macos::request_grants();
                        }
                        for (label, pane) in
                            [("Open Screen Recording", "Privacy_ScreenCapture"), ("Open Accessibility", "Privacy_Accessibility")]
                        {
                            if ui.button(label).clicked() {
                                let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
                                let _ = std::process::Command::new("open").arg(url).spawn();
                            }
                        }
                    });
                }
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}

fn fetch(rt: &tokio::runtime::Runtime, socket: &std::path::Path) -> Value {
    rt.block_on(ipc::send(socket, &Request::Info)).unwrap_or_else(|e| json!({ "error": e.to_string() }))
}

/// Whether the Agent refused only because the caller is not root (then an elevation prompt can fix it).
fn refused_for_root(reply: &Value) -> bool {
    reply["error"].as_str().is_some_and(|e| e.contains("root"))
}

/// Runs `rcli agent …` as root after the OS elevation prompt (polkit on Linux, an authorization dialog on
/// macOS) and returns its stdout. `stdin` travels over a pipe (Linux) or a 0600 temp file (macOS), never over
/// a command line, so the password is not exposed in `ps`.
// shortcut: no Windows UAC helper; root-only GUI actions show the Agent's "requires root" error until ticket 22.
#[cfg(target_os = "linux")]
fn elevated(args: &[&str], stdin: Option<&str>) -> anyhow::Result<String> {
    use std::io::Write;
    let mut cmd = std::process::Command::new("pkexec");
    cmd.arg(std::env::current_exe()?).args(args);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(out.status.success(), "{}", refusal(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned())
}

// shortcut: no Windows UAC helper; root-only GUI actions show the Agent's "requires root" error until ticket 22.
#[cfg(target_os = "macos")]
fn elevated(args: &[&str], stdin: Option<&str>) -> anyhow::Result<String> {
    let exe = shell_quote(&std::env::current_exe()?.to_string_lossy());
    let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
    let mut cmd = format!("{exe} {}", quoted.join(" "));
    let mut temp = None;
    if let Some(input) = stdin {
        let file = std::env::temp_dir().join(format!("rcli-gui-elevated-{}", std::process::id()));
        std::fs::write(&file, input)?;
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
        let path = shell_quote(&file.to_string_lossy());
        cmd = format!("{cmd} < {path}; rm -f {path}");
        temp = Some(file);
    }
    let script = format!("do shell script \"{cmd}\" with administrator privileges");
    let out = std::process::Command::new("osascript").arg("-e").arg(script).output();
    if let Some(file) = temp {
        let _ = std::fs::remove_file(file);
    }
    let out = out?;
    anyhow::ensure!(out.status.success(), "{}", refusal(&out.stderr));
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned())
}

/// What to show when an elevation helper failed: its stderr, or a clear line when the OS printed none (a cancelled
/// polkit or macOS authorization prompt leaves both streams empty, and an empty error reports nothing).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn refusal(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.is_empty() {
        "the elevation prompt was refused or failed".to_owned()
    } else {
        text.to_owned()
    }
}

/// Runs `rcli agent …` elevated after the UAC prompt and returns its stdout. The elevated process cannot inherit
/// this one's output, so a hidden `cmd.exe` writes stdout/stderr to temp files; `stdin` goes through a file too, so a
/// password never appears on the elevated command line.
#[cfg(windows)]
fn elevated(args: &[&str], stdin: Option<&str>) -> anyhow::Result<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
            UI::{
                Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
                WindowsAndMessaging::SW_HIDE,
            },
        },
    };

    let dir = std::env::temp_dir().join(format!("rcli-gui-elevated-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let (out_path, err_path, status_path) = (dir.join("out"), dir.join("err"), dir.join("status"));
    let batch = dir.join("run.cmd");
    // Every argument is quoted: a Server Key may contain `&` or `^`, and paths may contain spaces. cmd does not
    // expand `%` inside... it does, so a key with `%` would break; Update key alphabets are base64url, no `%`.
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut command = quote(&std::env::current_exe()?.to_string_lossy());
    command.push(' ');
    command.push_str(&args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
    if let Some(input) = stdin {
        let input_path = dir.join("stdin");
        std::fs::write(&input_path, input)?;
        command.push_str(&format!(" < {}", quote(&input_path.to_string_lossy())));
    }
    std::fs::write(
        &batch,
        format!(
            "@echo off\r\n{command} > {} 2> {}\r\necho %ERRORLEVEL% > {}\r\n",
            quote(&out_path.to_string_lossy()),
            quote(&err_path.to_string_lossy()),
            quote(&status_path.to_string_lossy())
        ),
    )?;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let file = wide(std::ffi::OsStr::new("cmd.exe"));
    let parameters = wide(std::ffi::OsStr::new(&format!("/c \"{}\"", batch.display())));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: windows::core::w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    let elevated = unsafe { ShellExecuteExW(&mut info) };
    if let Err(e) = elevated {
        let _ = std::fs::remove_dir_all(&dir);
        anyhow::bail!("elevation was refused or failed (UAC prompt): {e}");
    }
    anyhow::ensure!(!info.hProcess.is_invalid(), "elevation did not start a process");
    let mut process_code = 0u32;
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let _ = GetExitCodeProcess(info.hProcess, &mut process_code);
        let _ = CloseHandle(info.hProcess);
    }
    // cmd.exe's own exit code is the last command in the batch (`echo`), which always succeeds: the batch writes the
    // real one to the status file.
    let code = std::fs::read_to_string(&status_path).ok().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(process_code as i32);
    if code != 0 {
        let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
        let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        anyhow::bail!("{}", if stderr.trim().is_empty() { stdout.trim().to_owned() } else { stderr.trim().to_owned() });
    }
    let out = std::fs::read_to_string(&out_path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out.trim_end_matches(['\n', '\r']).to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn elevated(_args: &[&str], _stdin: Option<&str>) -> anyhow::Result<String> {
    anyhow::bail!("elevation is not supported here yet; run `sudo rcli agent password …`")
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcli_agent::platform::{Frame, Input, Inventory, PowerAction, Process, Pty, Resources, RunAs, SessionState, ShellOutput, UiElement};

/// The Agent's platform seen from a desktop: prompts (tickets 07/08) go to the GUI window over the IPC, and the
/// window opens when someone starts controlling the Device or a prompt needs the person's answer (ticket 14).
pub struct WindowPlatform {
    inner: Arc<dyn rcli_agent::platform::Platform>,
    prompts: Arc<ipc::Prompts>,
    open: Box<dyn Fn() + Send + Sync>,
}

/// Wraps the platform `rcli agent run` uses, on a desktop build.
pub fn wrap(
    inner: Arc<dyn rcli_agent::platform::Platform>,
    prompts: Arc<ipc::Prompts>,
    socket: PathBuf,
) -> Arc<dyn rcli_agent::platform::Platform> {
    let open = opener(socket, inner.clone());
    Arc::new(WindowPlatform { inner, prompts, open })
}

/// Opens the GUI window in a child `rcli` (no arguments), once; reopens it if the person closed it. A root service
/// opens it as the logged-in user, never as root: as root its password reveal would skip the elevation prompt.
/// With nobody logged in there is nobody to show it to.
fn opener(socket: PathBuf, platform: Arc<dyn rcli_agent::platform::Platform>) -> Box<dyn Fn() + Send + Sync> {
    let child: Arc<Mutex<Option<std::process::Child>>> = Default::default();
    let service_is_root = crate::upgrade::is_root();
    Box::new(move || {
        let mut child = child.lock().unwrap();
        if let Some(running) = child.as_mut() {
            if running.try_wait().ok().flatten().is_none() {
                return;
            }
        }
        let exe = std::env::current_exe().unwrap_or_else(|_| "rcli".into());
        if let Some(mut cmd) = gui_command(&socket, &exe, service_is_root, &platform) {
            *child = cmd.spawn().ok();
        }
    })
}

/// The command that opens the GUI window in the session that can show it, if this process can reach one.
fn gui_command(
    socket: &std::path::Path,
    exe: &std::path::Path,
    service_is_root: bool,
    platform: &Arc<dyn rcli_agent::platform::Platform>,
) -> Option<std::process::Command> {
    if !service_is_root {
        let mut cmd = std::process::Command::new(exe);
        cmd.env("RCLI_AGENT_SOCKET", socket);
        return Some(cmd);
    }
    let user = platform.metrics().ok().and_then(|r| r.logged_in_user).filter(|u| !u.is_empty() && u != "root");
    let user = user?;
    // shortcut: plain `sudo -u`; the session's display environment (DISPLAY / WAYLAND_DISPLAY) comes with the Linux
    // service installer of ticket 16.
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let mut cmd = std::process::Command::new("sudo");
        cmd.args(["-n", "-u", &user, "--", "env"]).arg(format!("RCLI_AGENT_SOCKET={}", socket.display())).arg(exe);
        Some(cmd)
    }
    // A launchd daemon sits outside every GUI session: `launchctl asuser` puts the window in the user's.
    #[cfg(target_os = "macos")]
    {
        let uid = std::process::Command::new("id").args(["-u", &user]).output().ok();
        let uid = uid.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).filter(|u| !u.is_empty())?;
        let mut cmd = std::process::Command::new("launchctl");
        cmd.args(["asuser", &uid, "sudo"])
            .args(["-n", "-u", &user, "--", "env"])
            .arg(format!("RCLI_AGENT_SOCKET={}", socket.display()))
            .arg(exe);
        Some(cmd)
    }
    // shortcut: a SYSTEM service cannot put a window on the console user's desktop without a `CreateProcessAsUser`
    // round trip through the session helper. Until that exists, prompts (root "ask", "password+accept") reject at
    // the Device because nobody can answer them; the control indicator is the tray-less window the person opens.
    #[cfg(windows)]
    {
        let _ = user;
        None
    }
}

impl rcli_agent::platform::Platform for WindowPlatform {
    fn hostname(&self) -> String {
        self.inner.hostname()
    }
    fn displays(&self) -> anyhow::Result<Vec<rcli_agent::platform::DisplayInfo>> {
        self.inner.displays()
    }
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        self.inner.capture(display)
    }
    /// Sending input is "someone is controlling the Device": the window is the control indicator.
    fn input(&self, input: Input) -> anyhow::Result<()> {
        (self.open)();
        self.inner.input(input)
    }
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        self.inner.accessibility(display)
    }
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        self.inner.recognize_text(frame)
    }
    fn session_state(&self) -> anyhow::Result<SessionState> {
        self.inner.session_state()
    }
    fn inventory(&self) -> anyhow::Result<Inventory> {
        self.inner.inventory()
    }
    fn metrics(&self) -> anyhow::Result<Resources> {
        self.inner.metrics()
    }
    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        self.inner.processes()
    }
    fn shell(&self, command: &str, run_as: RunAs, timeout: Duration) -> anyhow::Result<ShellOutput> {
        (self.open)();
        self.inner.shell(command, run_as, timeout)
    }
    fn pty(&self, run_as: RunAs, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        (self.open)();
        self.inner.pty(run_as, cols, rows)
    }
    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        self.inner.power(action)
    }
    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        self.inner.send_wake(mac)
    }
    fn install_update(&self, version: &str, build: &[u8]) -> anyhow::Result<()> {
        self.inner.install_update(version, build)
    }
    /// The Server's control indicator: on = someone is controlling the Device.
    fn set_control_indicator(&self, on: bool) -> anyhow::Result<()> {
        if on {
            (self.open)();
        }
        self.inner.set_control_indicator(on)
    }
    /// The GUI window shows the prompt and answers over the IPC; no answer in time rejects (the 30 s rule).
    fn ask_root(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        (self.open)();
        Ok(self.prompts.ask(ipc::PromptKind::Root, requester, timeout))
    }
    fn ask_accept(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        (self.open)();
        Ok(self.prompts.ask(ipc::PromptKind::Accept, requester, timeout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcli_agent::platform::{FakePlatform, Platform as _};

    #[test]
    fn prompts_go_to_the_gui_queue_not_the_platform_dialog() {
        let fake = Arc::new(FakePlatform::new("pc"));
        let prompts = Arc::new(ipc::Prompts::default());
        let platform = WindowPlatform { inner: fake.clone(), prompts: prompts.clone(), open: Box::new(|| {}) };
        let answerer = {
            let prompts = prompts.clone();
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    let listed = prompts.list();
                    if let Some(p) = listed.as_array().and_then(|a| a.first()) {
                        prompts.answer(p["id"].as_u64().unwrap(), true).unwrap();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                panic!("the prompt was never listed");
            })
        };
        assert!(platform.ask_root("alice", Duration::from_secs(5)).unwrap());
        answerer.join().unwrap();
        assert!(fake.root_prompts().is_empty(), "the platform's own prompt fired");
        // With nobody answering, the prompt auto-rejects when its time runs out.
        assert!(!platform.ask_accept("dev-laptop (10.0.0.5)", Duration::from_millis(50)).unwrap());
    }
}
