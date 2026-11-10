//! `rcli upgrade`: fetch the build the Server is rolling out for this OS/arch, verify it with the update key pinned
//! at `rcli setup`, and replace the running binary. As root on an installed Linux Device the service is restarted too.

use std::path::{Path, PathBuf};

use crate::config::Config;

pub struct Upgraded {
    pub version: String,
    pub target: String,
    pub path: std::path::PathBuf,
    pub restarted: bool,
}

pub async fn run(config: &Config, path: &Path, socket: Option<PathBuf>) -> anyhow::Result<Upgraded> {
    let key = hex::decode(config.update_key.trim()).unwrap_or_default();
    anyhow::ensure!(!key.is_empty(), "no update key is pinned; run `rcli setup` again");

    let target = rcli_agent::target();
    // A running Agent names this Device, so the Server serves the build rolled out to it; a Controller-only machine
    // has no Device row and gets the newest published build.
    let device = local_device(&socket.unwrap_or_else(rcli_agent::ipc::default_socket)).await;
    let url = format!("{}/api/v1/agent-builds/{target}", config.server.trim_end_matches('/'));
    let url = match &device {
        Some(device) => format!("{url}?device={device}"),
        None => url,
    };
    let resp = reqwest::Client::new().get(url).header("x-rcli-server-key", &config.server_key).send().await?;
    let status = resp.status();
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let version = header("x-rcli-version").unwrap_or_default();
    let signature = header("x-rcli-signature").and_then(|s| hex::decode(s).ok()).unwrap_or_default();
    if !status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body["error"]["message"].as_str().unwrap_or("no build for this OS/arch").to_owned();
        anyhow::bail!("{message}");
    }
    let build = resp.bytes().await?.to_vec();
    anyhow::ensure!(
        rcli_protocol::update::verify(&key, &version, &target, &build, &signature),
        "bad signature: the build is not signed with this machine's pinned update key; the old binary is untouched"
    );
    // Check the service first: on a root install, refusing after the swap would leave a new binary unstarted.
    let service = service_to_restart()?;
    replace(path, &build)?;
    let restarted = match service {
        Some(unit) => {
            restart(&unit)?;
            true
        }
        None => false,
    };
    Ok(Upgraded { version, target, path: path.to_owned(), restarted })
}

/// Replaces `path` atomically, so a crash mid-write never leaves a half-written binary behind.
// shortcut: renaming over a running .exe fails on Windows; ticket 22's installer must move-aside first.
fn replace(path: &Path, build: &[u8]) -> anyhow::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let temp = dir.join(format!(".{}.new", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&temp, build)?;
    #[cfg(unix)]
    std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

pub(crate) fn is_root() -> bool {
    std::process::Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

/// This machine's Device ID when its Agent is running: `rcli upgrade` then fetches the build rolled out to it.
async fn local_device(socket: &Path) -> Option<String> {
    let info = rcli_agent::ipc::send(socket, &rcli_agent::ipc::Request::Info).await.ok()?;
    Some(info["device_id"].as_str()?.to_owned())
}

/// Run as root on Linux, the binary being replaced is the installed Agent's: the unit to restart onto it. As a
/// non-root account (a Controller-only machine), or off Linux, there is none.
// shortcut: assumes the unit is named rcli-agent and runs the binary being replaced; ticket 16's
// `rcli agent install` records the real name and path, then this can read them instead. Not exercised here (no
// Linux systemd device); the ordering it relies on is: check the unit, then swap, then restart, so a refused
// check never leaves a new binary unstarted.
fn service_to_restart() -> anyhow::Result<Option<String>> {
    if !cfg!(target_os = "linux") || !is_root() {
        return Ok(None);
    }
    let unit = "rcli-agent";
    let installed = std::process::Command::new("systemctl").args(["cat", unit]).output();
    if !matches!(installed, Ok(ref o) if o.status.success()) {
        anyhow::bail!("no {unit} service is installed, so the new binary was not restarted");
    }
    Ok(Some(unit.into()))
}

fn restart(unit: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new("systemctl").args(["restart", unit]).output()?;
    anyhow::ensure!(out.status.success(), "restarting the {unit} service failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}
