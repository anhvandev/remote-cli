//! The Agent GUI window (ticket 14): a small egui window, a client of the Agent's local IPC.
//! Built only with `--features gui`; the default `rcli` binary links no display libraries.

use std::path::PathBuf;

use eframe::egui;
use rcli_agent::ipc;

/// Opens the GUI window. `socket` overrides the Agent's IPC socket (`RCLI_AGENT_SOCKET`).
pub fn run(socket: Option<PathBuf>) -> anyhow::Result<()> {
    let socket = socket.unwrap_or_else(ipc::default_socket);
    let rt = tokio::runtime::Runtime::new()?;
    let info = rt.block_on(ipc::request(&socket, "info")).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }));
    eframe::run_native("rcli", eframe::NativeOptions::default(), Box::new(move |_| Ok(Box::new(App { info }))))
        .map_err(|e| anyhow::anyhow!("cannot open the GUI: {e}"))?;
    Ok(())
}

struct App {
    /// The Agent's `info` reply: `device_id`, `connected`, `server`, `controllers`.
    info: serde_json::Value,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rcli");
            if let Some(error) = self.info["error"].as_str() {
                ui.label(format!("Agent unreachable: {error}"));
                return;
            }
            ui.label(format!("Device ID: {}", self.info["device_id"].as_str().unwrap_or("-")));
        });
    }
}
