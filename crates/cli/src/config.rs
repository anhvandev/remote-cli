//! Controller configuration: the per-user config directory holds the Server Address, the Server Key and the pinned
//! update key; remembered Device Passwords live in the OS keychain (Keychain, Credential Manager, Secret Service),
//! with a `0600` file fallback.

use std::path::{Path, PathBuf};

use rcli_agent::secret_store::SecretStore;
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
    pub session: Option<String>,
}

/// The platform's per-user config directory; `RCLI_CONFIG_DIR` overrides it (tests).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RCLI_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    platform_dir()
}

/// The platform's standard per-user config directory.
fn platform_dir() -> PathBuf {
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
pub fn set_session(dir: &Path, session: Option<String>) -> anyhow::Result<Config> {
    let mut config = load_at(dir)?.ok_or_else(|| anyhow::anyhow!("run `rcli setup` first (no saved Server Address)"))?;
    config.session = session;
    save(dir, &config)?;
    Ok(config)
}

/// This Controller's name, as audit entries and the Control Lock holder show it.
pub fn hostname() -> String {
    hostname_or("controller")
}

/// The local host name with a caller-chosen last resort: the Controller's name above, or the Device name an install
/// records (`windows_install::device_name`). One probe serves both (S7).
pub fn hostname_or(fallback: &str) -> String {
    let env = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok();
    let command = std::process::Command::new("hostname").output().ok().map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    hostname_from(env, command, fallback)
}

/// The probe's decision, split out so the fallbacks are tested without changing this process's environment.
fn hostname_from(env: Option<String>, command: Option<String>, fallback: &str) -> String {
    env.filter(|name| !name.trim().is_empty())
        .or(command.filter(|name| !name.trim().is_empty()))
        .unwrap_or_else(|| fallback.to_owned())
}

/// The Server Key's secret and Device Passwords travel in request headers, so the Controller API must be reached over
/// https (it sits behind a TLS reverse proxy). The one exception is loopback, for a Server on this computer and for
/// tests: nothing leaves the computer there. There is no flag to accept plain http to another host.
pub(crate) fn check_transport(base: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(base).map_err(|e| anyhow::anyhow!("bad Server Address {base}: {e}"))?;
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    let loopback = host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    match url.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        "http" => anyhow::bail!(
            "{base} is plain http: the Server Key and Device Passwords would travel unencrypted. Use the https:// \
             address of your TLS reverse proxy"
        ),
        other => anyhow::bail!("the Server Address must start with https:// (not {other}://)"),
    }
}

/// `rcli setup`: saves the Server Address, Server Key and the pinned update key.
pub async fn setup(dir: &Path, server: &str, key: &str) -> anyhow::Result<String> {
    let server_key = rcli_protocol::server_key::ServerKey::parse(key)
        .ok_or_else(|| anyhow::anyhow!("the Server Key must look like rk_<fingerprint>_<secret>"))?;
    let base = server.trim().trim_end_matches('/');
    check_transport(base)?;
    let http = reqwest::Client::new();
    // A key for another Server is caught here. The CA is public, so this proves nothing about who answers: the
    // update key's HMAC under the secret does that, and https protects the secret on the way (see ADR 0003).
    let ca = http.get(format!("{base}/api/v1/server-ca")).send().await?.error_for_status()?.text().await?;
    anyhow::ensure!(
        rcli_agent::ca_fingerprint(&ca)? == server_key.fingerprint,
        "the Server's CA fingerprint does not match the Server Key"
    );
    let update: serde_json::Value = http.get(format!("{base}/api/v1/update-key")).send().await?.error_for_status()?.json().await?;
    let update_key = update["update_key"].as_str().unwrap_or_default().to_owned();
    check_pin(&server_key.secret, &update_key, update["mac"].as_str())?;
    let config = Config {
        server: base.to_owned(),
        server_key: key.trim().to_owned(),
        update_key,
        session: None,
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

/// A remembered Device Password and the Controller's name it was saved with.
#[derive(Serialize, Deserialize)]
pub struct Remembered {
    pub password: String,
    pub controller: String,
}

/// Remembered Device Passwords by Device ID, under `<config dir>/passwords` (0600 files). Each holds the password
/// itself, or `IN_KEYCHAIN` when the OS keychain took it, so the Device IDs stay listable either way.
fn password_store(dir: &Path) -> rcli_agent::secret_store::FileSecretStore {
    rcli_agent::secret_store::FileSecretStore::new(dir.join("passwords"))
}

const IN_KEYCHAIN: &str = "keychain";

/// The OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service), used for the platform's own
/// config directory. A config directory chosen with `--config-dir` / `RCLI_CONFIG_DIR` (tests, portable setups)
/// keeps everything in it.
// The Linux Secret Service needs a D-Bus session, which a headless Controller or a cron job lacks: `remember`
// falls back to the 0600 file when `set` fails, so such a Controller keeps working without a keychain.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod keychain {
    fn entry(device: &str) -> keyring::Result<keyring::Entry> {
        keyring::Entry::new("rcli", device)
    }

    pub fn set(device: &str, value: &str) -> anyhow::Result<()> {
        Ok(entry(device)?.set_password(value)?)
    }

    pub fn get(device: &str) -> anyhow::Result<Option<String>> {
        match entry(device)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn remove(device: &str) {
        let _ = entry(device).and_then(|e| e.delete_credential());
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod keychain {
    pub fn set(_device: &str, _value: &str) -> anyhow::Result<()> {
        anyhow::bail!("no OS keychain here")
    }

    pub fn get(_device: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    pub fn remove(_device: &str) {}
}

/// A plaintext password file from before the passwords were encrypted; `remember`/`forget` delete it.
fn legacy_password_path(dir: &Path, device: &str) -> PathBuf {
    dir.join("passwords").join(format!("{device}.json"))
}

/// Remembers a Device Password: in the OS keychain for the platform's config directory when it takes it, else in
/// the 0600 file.
pub fn remember(dir: &Path, device: &str, password: &str, controller: &str) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(legacy_password_path(dir, device));
    let remembered = serde_json::to_string(&Remembered { password: password.into(), controller: controller.into() })?;
    let in_keychain = dir == platform_dir() && keychain::set(device, &remembered).is_ok();
    password_store(dir).set(device, if in_keychain { IN_KEYCHAIN } else { &remembered })
}

fn root_path(dir: &Path, device: &str) -> PathBuf {
    dir.join("root").join(format!("{device}.json"))
}

/// Whether `rcli root on <Device ID>` made `--as root` the default for this Device's exec and shell.
pub fn root_default(dir: &Path, device: &str) -> bool {
    std::fs::read(root_path(dir, device)).is_ok_and(|b| serde_json::from_slice::<bool>(&b).unwrap_or(false))
}

pub fn set_root(dir: &Path, device: &str, on: bool) -> anyhow::Result<()> {
    let path = root_path(dir, device);
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_private(&path, &serde_json::to_vec(&on)?)
}

pub fn remembered(dir: &Path, device: &str) -> anyhow::Result<Option<Remembered>> {
    let stored = match password_store(dir).get(device)? {
        // The file only says "in the keychain": a Controller that can no longer reach it (no D-Bus session, a
        // different account) gets a message naming the keychain, not a bare D-Bus error.
        Some(stored) if stored == IN_KEYCHAIN => keychain::get(device)
            .map_err(|e| anyhow::anyhow!("cannot read the remembered Device Password from the OS keychain: {e}"))?,
        stored => stored,
    };
    stored.map(|s| serde_json::from_str(&s)).transpose().map_err(Into::into)
}

/// Any Device ID with a remembered password (cheapest is fine: the Server scopes the answer anyway).
pub fn some_remembered(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir.join("passwords")).ok()?;
    entries.flatten().find_map(|e| {
        let name = e.file_name();
        name.to_str()?.strip_prefix('.').filter(|n| *n != "secrets.key").map(str::to_owned)
    })
}

pub fn forget(dir: &Path, device: &str) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(legacy_password_path(dir, device));
    if password_store(dir).get(device)?.as_deref() == Some(IN_KEYCHAIN) {
        keychain::remove(device);
    }
    password_store(dir).remove(device)
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

/// Authenticates `req` as a password connection: the Server Key, the Device ID and its Device Password, and the
/// The Controller's name.
pub fn password_connection(
    req: reqwest::RequestBuilder,
    key: &str,
    device: &str,
    password: &str,
    controller: &str,
) -> reqwest::RequestBuilder {
    req.header("x-rcli-server-key", key)
        .header("x-rcli-device", device)
        .header("x-rcli-device-password", password)
        .header("x-rcli-controller", controller)
}

/// `GET .../devices/{id}` with the password headers: the check `rcli connect` makes before saving.
pub async fn verify(server: &str, key: &str, device: &str, password: &str, controller: &str) -> anyhow::Result<()> {
    let req = reqwest::Client::new().get(format!("{}/api/v1/devices/{}", server.trim_end_matches('/'), device));
    let resp = password_connection(req, key, device, password, controller).send().await?;
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_server_address_must_be_https_unless_it_is_loopback() {
        use super::check_transport as ok;
        assert!(ok("https://server.example.com:8080").is_ok());
        // Loopback http is the one exception: a Server on this computer, and the tests.
        for local in ["http://127.0.0.1:8080", "http://localhost:8080", "http://[::1]:8080"] {
            assert!(ok(local).is_ok(), "{local}");
        }
        let err = ok("http://server.example.com:8080").unwrap_err().to_string();
        assert!(err.contains("TLS reverse proxy"), "{err}");
        assert!(!err.contains("--insecure-http"), "the flag is gone, so the error cannot offer it: {err}");
        assert!(ok("ftp://server.example.com").is_err());
    }

    /// A headless Linux Controller has no Secret Service (no D-Bus session): the call must fail cleanly so
    /// `remember` takes the 0600 file fallback, never panic or hang. A bogus bus address keeps this from touching a
    /// real keychain, even when the suite runs on a desktop.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_keychain_without_a_secret_service_fails_cleanly() {
        let previous = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/rcli-test-bus");
        let set = super::keychain::set("rcli-test-device", "value");
        let get = super::keychain::get("rcli-test-device");
        match previous {
            Some(address) => std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address),
            None => std::env::remove_var("DBUS_SESSION_BUS_ADDRESS"),
        }
        assert!(set.is_err(), "storing must report the unreachable Secret Service: {set:?}");
        assert!(get.as_ref().map_or(true, Option::is_none), "reading must not invent a password: {get:?}");
    }

    #[test]
    fn one_hostname_probe_falls_back_in_order() {
        use super::hostname_from as pick;
        assert_eq!(pick(Some("lab-pc".into()), Some("ignored".into()), "controller"), "lab-pc");
        assert_eq!(pick(Some("  ".into()), Some("from-command".into()), "controller"), "from-command");
        assert_eq!(pick(None, Some("  ".into()), "controller"), "controller");
        assert_eq!(pick(None, None, "windows"), "windows");
        assert!(!super::hostname().trim().is_empty(), "the Controller always has a name");
    }

    #[test]
    fn a_remembered_password_is_encrypted_at_rest_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("rcli-remembered-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::remember(&dir, "123456789", "hunter2xyz", "dev-laptop (10.0.0.5)").unwrap();
        let raw: Vec<Vec<u8>> = std::fs::read_dir(dir.join("passwords"))
            .unwrap()
            .flatten()
            .map(|e| std::fs::read(e.path()).unwrap())
            .collect();
        assert!(raw.iter().all(|b| !b.windows(10).any(|w| w == b"hunter2xyz")), "the password is not stored in plaintext");
        let remembered = super::remembered(&dir, "123456789").unwrap().unwrap();
        assert_eq!(remembered.password, "hunter2xyz");
        assert_eq!(remembered.controller, "dev-laptop (10.0.0.5)");
        assert_eq!(super::some_remembered(&dir).as_deref(), Some("123456789"));
        super::forget(&dir, "123456789").unwrap();
        assert!(super::remembered(&dir, "123456789").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
