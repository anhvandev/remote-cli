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
    /// The prompts waiting for the person at the Device (tickets 07/08).
    prompts: Value,
    /// The new Device Password being typed.
    password: String,
    /// The Device Password after a reveal (root), for the Copy button.
    shown: Option<String>,
    /// The Server Key and unlock password after an elevated reveal.
    secrets: Option<Value>,
    /// The Device Settings editor, seeded from `info` on the first fetch.
    terminal_user: String,
    root: String,
    accept: String,
    /// The new unlock password being typed.
    unlock: String,
    /// The last action's message.
    note: String,
}

impl App {
    fn new(socket: PathBuf) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let info = fetch(&rt, &socket);
        let settings = &info["settings"];
        let terminal_user = settings["terminal_user"].as_str().unwrap_or_default().to_owned();
        let root = settings["root"].as_str().unwrap_or("ask").to_owned();
        let accept = settings["accept"].as_str().unwrap_or("password").to_owned();
        Self {
            socket,
            rt,
            info,
            prompts: json!([]),
            password: String::new(),
            shown: None,
            secrets: None,
            terminal_user,
            root,
            accept,
            unlock: String::new(),
            note: String::new(),
        }
    }

    /// One IPC round-trip; an `error` reply comes back as `{"error": ...}`.
    fn ipc(&self, body: Value) -> Value {
        let socket = self.socket.clone();
        match self.rt.block_on(ipc::send(&socket, body)) {
            Ok(reply) => reply,
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// One IPC request, run elevated when the Agent refuses only for lack of root.
    fn ipc_elevated(&self, body: Value, args: &[&str], stdin: Option<&str>) -> Value {
        let reply = self.ipc(body);
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

    /// Applies the Device Settings editors (root; the Agent saves and applies them at once).
    fn set_settings(&mut self) {
        let calls = [
            (
                json!({ "command": "terminal-user.set", "user": self.terminal_user }),
                vec!["agent", "terminal-user", "set", &self.terminal_user],
            ),
            (json!({ "command": "root.set", "root": self.root }), vec!["agent", "root", &self.root]),
            (json!({ "command": "accept.set", "accept": self.accept }), vec!["agent", "accept", &self.accept]),
        ];
        for (body, args) in calls {
            let reply = self.ipc_elevated(body, &args, None);
            self.note = note(&reply);
        }
    }

    /// Stores a new unlock password (root; over stdin, never on a command line).
    fn set_unlock(&mut self) {
        let reply = self.ipc_elevated(
            json!({ "command": "unlock-password.set", "password": self.unlock }),
            &["agent", "unlock-password", "set"],
            Some(&self.unlock),
        );
        self.note = note(&reply);
        self.unlock.clear();
    }

    fn clear_unlock(&mut self) {
        let reply = self.ipc_elevated(json!({ "command": "unlock-password.clear" }), &["agent", "unlock-password", "clear"], None);
        self.note = note(&reply);
    }

    /// The Server Key and the unlock password: root-only fields of `info`, so this always asks the OS (like Reveal).
    fn reveal_secrets(&mut self) {
        let reply = match elevated(&["agent", "info", "--json"], None) {
            Ok(out) => serde_json::from_str::<Value>(&out).unwrap_or_else(|_| json!({ "error": out })),
            Err(e) => json!({ "error": e.to_string() }),
        };
        if reply.get("server_key").is_some() {
            self.secrets = Some(reply);
        } else {
            self.note = format!("cannot read the Server Key: {}", reply["error"].as_str().unwrap_or_default());
        }
    }

    /// Grants root to one Controller for this session only (root).
    fn grant_root(&mut self, who: &str) {
        let reply = self.ipc_elevated(json!({ "command": "root.grant", "who": who }), &["agent", "root", "grant", who], None);
        self.note = note(&reply);
    }

    /// The person at the Device's answer to a waiting prompt. Accepting needs root (the OS prompt);
    /// refusing is open to anyone and goes straight over the IPC.
    fn answer_prompt(&mut self, id: u64, accept: bool) {
        let body = json!({ "command": "prompt.answer", "id": id, "accept": accept });
        let reply = if accept {
            let id = id.to_string();
            self.ipc_elevated(body, &["agent", "prompt", "answer", &id, "accept"], None)
        } else {
            self.ipc(body)
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
        self.prompts = self.ipc(json!({ "command": "prompts" }));
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
                if ui.button("Reveal Server Key and unlock password").clicked() {
                    self.reveal_secrets();
                }
                if let Some(secrets) = &self.secrets {
                    ui.label(format!("Server Key: {}", secrets["server_key"].as_str().unwrap_or("-")));
                }
                ui.separator();
                ui.label("Device Settings");
                let s = &self.info["settings"];
                ui.label(format!("Terminal user: {}", s["terminal_user"].as_str().unwrap_or("-")));
                ui.label(format!("Root: {}", s["root"].as_str().unwrap_or("-")));
                ui.label(format!("Accept: {}", s["accept"].as_str().unwrap_or("-")));
                ui.horizontal(|ui| {
                    ui.label("Terminal user:");
                    ui.text_edit_singleline(&mut self.terminal_user);
                });
                ui.horizontal(|ui| {
                    ui.label("Root:");
                    for level in ["never", "ask", "always"] {
                        ui.selectable_value(&mut self.root, level.to_owned(), level);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Accept:");
                    for mode in ["password", "password+accept"] {
                        ui.selectable_value(&mut self.accept, mode.to_owned(), mode);
                    }
                });
                if ui.button("Apply Device Settings").clicked() {
                    self.set_settings();
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
                    ui.label(format!(
                        "Unlock password: {}",
                        secrets["unlock_password"].as_str().unwrap_or("(not set)")
                    ));
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
                            let reply = self.ipc(json!({ "command": "disconnect", "who": who }));
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
                ui.label("macOS permissions (Live View and remote input need them)");
                ui.label("System Settings > Privacy & Security > Screen Recording: allow rcli.");
                ui.label("System Settings > Privacy & Security > Accessibility: allow rcli.");
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

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcli_agent::platform::{
    Frame, Input, Inventory, PowerAction, Process, Pty, Resources, RunAs, SessionState, ShellOutput, UiElement,
};

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
    Arc::new(WindowPlatform { inner, prompts, open: opener(socket) })
}

/// Opens the GUI window in a child `rcli` (no arguments), once; reopens it if the person closed it.
fn opener(socket: PathBuf) -> Box<dyn Fn() + Send + Sync> {
    let child: Arc<Mutex<Option<std::process::Child>>> = Default::default();
    Box::new(move || {
        let mut child = child.lock().unwrap();
        if let Some(running) = child.as_mut() {
            if running.try_wait().ok().flatten().is_none() {
                return;
            }
        }
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "rcli".into()));
        cmd.env("RCLI_AGENT_SOCKET", &socket);
        *child = cmd.spawn().ok();
    })
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
        Ok(self.prompts.ask("root", requester, timeout))
    }
    fn ask_accept(&self, requester: &str, timeout: Duration) -> anyhow::Result<bool> {
        (self.open)();
        Ok(self.prompts.ask("accept", requester, timeout))
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
