//! Controller configuration: the per-user config directory holds the Server Address, the Server Key and the pinned
//! update key; remembered Device Passwords live in the OS keychain, with a `0600` file fallback.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

/// `<config dir>/config.json`.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub server: String,
    pub server_key: String,
    /// Hex Ed25519 update key pinned at `rcli setup`.
    #[serde(default)]
    pub update_key: String,
    /// User session from `rcli login` (ticket 05).
    #[serde(default)]
    pub token: Option<String>,
}

/// The platform's per-user config directory; `RCLI_CONFIG_DIR` overrides it (tests).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RCLI_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    #[cfg(target_os = "windows")]
    return std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("rcli");
    #[cfg(target_os = "macos")]
    return std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Library/Application Support/rcli");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("rcli")
}

pub fn load() -> anyhow::Result<Option<Config>> {
    load_at(&dir())
}

pub fn load_at(dir: &Path) -> anyhow::Result<Option<Config>> {
    match std::fs::read(dir.join("config.json")) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn save(dir: &Path, config: &Config) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("config.json");
    std::fs::write(&path, serde_json::to_vec_pretty(config)?)?;
    // The file holds the User session, so keep it private even if an older `rcli setup` created it world-readable.
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

/// Stores (or with `None` clears) the User session from `rcli login` / `rcli logout`.
pub fn set_token(dir: &Path, token: Option<String>) -> anyhow::Result<Config> {
    let mut config = load_at(dir)?.ok_or_else(|| anyhow::anyhow!("run `rcli setup` first (no saved Server Address)"))?;
    config.token = token;
    save(dir, &config)?;
    Ok(config)
}

/// This Controller machine's name, as audit entries and the Control Lock holder show it.
pub fn hostname() -> String {
    let var = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"));
    var.ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        })
        .unwrap_or_else(|| "controller".into())
}

/// `rcli setup`: saves the Server Address, Server Key and the pinned update key.
pub async fn setup(dir: &Path, server: &str, key: &str) -> anyhow::Result<String> {
    let fingerprint = key
        .trim()
        .strip_prefix("rk_")
        .and_then(|rest| rest.split_once('_'))
        .map(|(fingerprint, secret)| (fingerprint.to_owned(), secret.to_owned()))
        .filter(|(fingerprint, secret)| fingerprint.len() == 64 && !secret.is_empty())
        .ok_or_else(|| anyhow::anyhow!("the Server Key must look like rk_<fingerprint>_<secret>"))?;
    let base = server.trim().trim_end_matches('/');
    let http = reqwest::Client::new();
    // The Server Key's fingerprint authenticates the Server: the update key is only pinned when the CA matches.
    let ca = http.get(format!("{base}/api/v1/server-ca")).send().await?.error_for_status()?.text().await?;
    let ca_der = ca_der(&ca)?;
    use sha2::Digest;
    anyhow::ensure!(
        hex::encode(sha2::Sha256::digest(&ca_der)) == fingerprint.0.to_lowercase(),
        "the Server's CA fingerprint does not match the Server Key"
    );
    let update: serde_json::Value = http.get(format!("{base}/api/v1/update-key")).send().await?.error_for_status()?.json().await?;
    let update_key = update["update_key"].as_str().unwrap_or_default().to_owned();
    check_pin(&fingerprint.1, &update_key, update["mac"].as_str())?;
    let config = Config {
        server: base.to_owned(),
        server_key: key.trim().to_owned(),
        update_key,
        token: None,
    };
    save(dir, &config)?;
    Ok(format!("saved the Server {} and its key\n", config.server))
}

/// The update key must come with the HMAC under the Server Key's secret, or an impostor could swap it in.
fn check_pin(server_key_secret: &str, update_key_hex: &str, mac: Option<&str>) -> anyhow::Result<()> {
    anyhow::ensure!(
        mac == Some(hex::encode(rcli_protocol::update::pin_mac(server_key_secret, update_key_hex)).as_str()),
        "the Server did not prove the update key with this Server Key; refusing to pin it"
    );
    Ok(())
}

fn ca_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    Ok(base64::engine::general_purpose::STANDARD.decode(body.trim())?)
}

/// A remembered Device Password and the Controller machine name it was saved with.
#[derive(Serialize, Deserialize)]
pub struct Remembered {
    pub password: String,
    pub controller: String,
}

fn password_path(dir: &Path, device: &str) -> PathBuf {
    dir.join("passwords").join(format!("{device}.json"))
}

/// shortcut: the `0600` file fallback only; a later ticket can put this in the OS keychain without changing callers.
pub fn remember(dir: &Path, device: &str, password: &str, controller: &str) -> anyhow::Result<()> {
    let path = password_path(dir, device);
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_private(&path, &serde_json::to_vec(&Remembered { password: password.into(), controller: controller.into() })?)
}

pub fn remembered(dir: &Path, device: &str) -> anyhow::Result<Option<Remembered>> {
    match std::fs::read(password_path(dir, device)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Any Device ID with a remembered password (cheapest is fine: the Server scopes the answer anyway).
pub fn some_remembered(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir.join("passwords")).ok()?;
    entries.flatten().find_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_owned))
}

pub fn forget(dir: &Path, device: &str) -> anyhow::Result<()> {
    match std::fs::remove_file(password_path(dir, device)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(path)?.write_all(bytes)?;
    Ok(())
}

/// `GET .../devices/{id}` with the password headers: the check `rcli connect` makes before saving.
pub async fn verify(server: &str, key: &str, device: &str, password: &str, controller: &str) -> anyhow::Result<()> {
    let resp = reqwest::Client::new()
        .get(format!("{}/api/v1/devices/{}", server.trim_end_matches('/'), device))
        .header("x-rcli-server-key", key)
        .header("x-rcli-device", device)
        .header("x-rcli-device-password", password)
        .header("x-rcli-controller", controller)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let message = body["error"]["message"].as_str().unwrap_or("unauthorized");
    // A lockout is not a wrong password: say how long it lasts instead of denying credentials that may be right.
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("{message}");
    }
    anyhow::bail!("the Device ID or Device Password is wrong: {message}")
}

/// The JSON `rcli setup --json` prints.
pub fn json(config: &Config) -> serde_json::Value {
    json!({ "server": config.server, "server_key": config.server_key, "update_key": config.update_key })
}
