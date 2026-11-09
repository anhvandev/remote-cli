//! The Agent GUI window (ticket 14): a small egui window, a client of the Agent's local IPC.
//! Built only with `--features gui`; the default `rcli` binary links no display libraries.

use std::path::PathBuf;

use eframe::egui;
use rcli_agent::ipc;
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
    /// The new Device Password being typed.
    password: String,
    /// The Device Password after a reveal (root), for the Copy button.
    shown: Option<String>,
    /// The last action's message.
    note: String,
}

impl App {
    fn new(socket: PathBuf) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let info = fetch(&rt, &socket);
        Self { socket, rt, info, password: String::new(), shown: None, note: String::new() }
    }

    /// One IPC round-trip; an `error` reply comes back as `{"error": ...}`.
    fn ipc(&self, body: Value) -> Value {
        let socket = self.socket.clone();
        match self.rt.block_on(ipc::send(&socket, body)) {
            Ok(reply) => reply,
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    fn reveal(&mut self) {
        let reply = self.ipc(json!({ "command": "password.show" }));
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
        let reply = self.ipc(json!({ "command": "password.set", "password": self.password }));
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.info = fetch(&self.rt, &self.socket);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rcli");
            if !self.note.is_empty() {
                ui.label(&self.note);
            }
            if let Some(error) = self.info["error"].as_str() {
                ui.label(format!("Agent unreachable: {error}"));
            } else {
                ui.label(format!("Device ID: {}", self.info["device_id"].as_str().unwrap_or("-")));
                ui.label(format!(
                    "Connection: {}",
                    if self.info["connected"] == true { "connected" } else { "disconnected" }
                ));
                ui.label(format!("Server: {}", self.info["server"].as_str().unwrap_or("-")));
                ui.separator();
                ui.label("Device Settings");
                let s = &self.info["settings"];
                ui.label(format!("Terminal user: {}", s["terminal_user"].as_str().unwrap_or("-")));
                ui.label(format!("Root: {}", s["root"].as_str().unwrap_or("-")));
                ui.label(format!("Accept: {}", s["accept"].as_str().unwrap_or("-")));
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
                            let reply = self.ipc(json!({ "command": "disconnect", "who": who }));
                            self.note = reply["message"].as_str().unwrap_or_default().to_owned();
                        }
                    });
                }
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}

fn fetch(rt: &tokio::runtime::Runtime, socket: &std::path::Path) -> Value {
    rt.block_on(ipc::request(socket, "info")).unwrap_or_else(|e| json!({ "error": e.to_string() }))
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
    anyhow::ensure!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr).trim());
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
    anyhow::ensure!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn elevated(_args: &[&str], _stdin: Option<&str>) -> anyhow::Result<String> {
    anyhow::bail!("elevation is not supported here yet; run `sudo rcli agent password …`")
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
