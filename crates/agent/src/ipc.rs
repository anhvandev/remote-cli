//! Local IPC: the running Agent serves `rcli agent ...` on a Unix socket (a named pipe on Windows later).
//! One JSON request per connection, one JSON reply, newline-terminated; only `tokio::io` traits are used, so a
//! pipe transport can reuse `handle`.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use rcli_protocol::{
    agent::IdentityUpdate,
    password::MIN_LENGTH,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, watch},
};

use crate::{secret_store::SecretStore, ServerEndpoint};

/// The Device Password's name in the Secret Store.
pub const DEVICE_PASSWORD: &str = "device_password";

/// How long a change may wait for the Server's answer.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(10);

/// A Device identity change that needs the Server's acknowledgement, sent on the live connection.
pub struct Change {
    pub update: IdentityUpdate,
    /// The Device ID in effect after the change, or the message to show.
    pub reply: oneshot::Sender<Result<String, String>>,
}

/// What the IPC clients see of the running Agent.
pub struct Status {
    device_id: RwLock<String>,
    /// The Server the Agent connects to; a change reconnects it without a restart.
    endpoint: watch::Sender<ServerEndpoint>,
    connected: AtomicBool,
    secrets: Arc<dyn SecretStore>,
    /// Where `rcli agent run` keeps its credentials, so a new Device ID survives a restart (None in tests).
    credentials: Mutex<Option<PathBuf>>,
    changes: mpsc::UnboundedSender<Change>,
    changes_rx: Mutex<Option<mpsc::UnboundedReceiver<Change>>>,
    next_request: AtomicU64,
}

impl Status {
    pub fn new(device_id: &str, endpoint: ServerEndpoint, secrets: Arc<dyn SecretStore>) -> Self {
        let (changes, changes_rx) = mpsc::unbounded_channel();
        Self {
            device_id: RwLock::new(device_id.into()),
            endpoint: watch::channel(endpoint).0,
            connected: AtomicBool::new(false),
            secrets,
            credentials: Mutex::new(None),
            changes,
            changes_rx: Mutex::new(Some(changes_rx)),
            next_request: AtomicU64::new(0),
        }
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn device_id(&self) -> String {
        self.device_id.read().unwrap().clone()
    }

    pub fn endpoint(&self) -> ServerEndpoint {
        self.endpoint.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ServerEndpoint> {
        self.endpoint.subscribe()
    }

    /// Points the Agent at another Server: the running session reconnects with the new values.
    pub fn set_endpoint(&self, endpoint: ServerEndpoint) {
        self.endpoint.send_replace(endpoint);
    }

    /// Tells the Agent to rewrite `credentials.json` (so a new Device ID survives a restart).
    pub fn set_credentials_path(&self, path: PathBuf) {
        *self.credentials.lock().unwrap() = Some(path);
    }

    /// The change receiver, taken once by `run`.
    pub fn take_changes(&self) -> Option<mpsc::UnboundedReceiver<Change>> {
        self.changes_rx.lock().unwrap().take()
    }

    /// Stores a Device Password the Server set from the Dashboard.
    pub fn store_password(&self, password: &str) -> anyhow::Result<()> {
        self.secrets.set(DEVICE_PASSWORD, password)
    }

    pub fn set_device_id(&self, device_id: &str) {
        *self.device_id.write().unwrap() = device_id.to_owned();
        let path = self.credentials.lock().unwrap().clone();
        let Some(path) = path else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        let Ok(mut creds) = serde_json::from_slice::<crate::Credentials>(&bytes) else { return };
        creds.device_id = device_id.to_owned();
        if let Ok(bytes) = serde_json::to_vec(&creds) {
            let _ = std::fs::write(&path, bytes);
        }
    }

    /// Sends a change to the Server and waits for its answer; `Err` is the message to show.
    async fn identity(&self, password: Option<&str>, device_id: Option<&str>, name: Option<&str>) -> Result<String, String> {
        if !self.connected.load(Ordering::Relaxed) {
            return Err("the Agent is not connected to the Server; try again once it is Online".into());
        }
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        let update = IdentityUpdate {
            request_id,
            device_password: password.unwrap_or_default().into(),
            device_id: device_id.unwrap_or_default().into(),
            name: name.unwrap_or_default().into(),
        };
        let (reply, answer) = oneshot::channel();
        if self.changes.send(Change { update, reply }).is_err() {
            return Err("the Agent is not running".into());
        }
        match tokio::time::timeout(CHANGE_TIMEOUT, answer).await {
            Ok(Ok(result)) => result,
            // shortcut: a connection lost after the Server applied the change also lands here; re-read `rcli agent info`
            // to see the Device ID. Add an idempotent re-check if that race ever shows up.
            Ok(Err(_)) | Err(_) => Err("the Agent did not get an answer from the Server; check `rcli agent info`".into()),
        }
    }

    fn info(&self) -> Value {
        json!({
            "device_id": self.device_id(),
            "server": self.endpoint.borrow().addr,
            "connected": self.connected.load(Ordering::Relaxed),
        })
    }
}

/// Where the Agent listens when no other path is given.
pub fn default_socket() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/var/run/rcli/agent.sock");
    #[cfg(not(target_os = "macos"))]
    PathBuf::from("/run/rcli/agent.sock")
}

// shortcut: Unix socket only; a Windows named-pipe `serve`/`request` (same `handle`) lands with the Windows ticket.
#[cfg(unix)]
pub async fn serve(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    // Only root may read the Device Password or change the Device; `info` is for any local user.
    serve_with(path, status, |stream| stream.peer_cred().is_ok_and(|c| c.uid() == 0)).await
}

/// Tests run unprivileged: this stands in for "the caller is root", nothing else.
#[cfg(unix)]
pub async fn serve_trusting_all(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    serve_with(path, status, |_| true).await
}

#[cfg(unix)]
async fn serve_with(
    path: PathBuf,
    status: Arc<Status>,
    is_root: impl Fn(&tokio::net::UnixStream) -> bool + Send + Sync + 'static,
) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    let is_root = Arc::new(is_root);
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let status = status.clone();
        let is_root = is_root(&stream);
        tokio::spawn(async move {
            if let Err(e) = handle(stream, status, is_root).await {
                tracing::debug!("IPC client ended: {e}");
            }
        });
    }
}

/// Answers one request on `conn`. `is_root` is the caller's peer credential.
async fn handle<S>(conn: S, status: Arc<Status>, is_root: bool) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(conn);
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let request: Value = serde_json::from_str(&line)?;
    let reply = dispatch(&status, &request, is_root).await;
    write.write_all(format!("{reply}\n").as_bytes()).await?;
    write.flush().await?;
    Ok(())
}

/// The reply to one request. Any local user may read the Device ID and status; everything else is root-only.
async fn dispatch(status: &Status, request: &Value, is_root: bool) -> Value {
    let command = request["command"].as_str();
    if command == Some("info") {
        return status.info();
    }
    if !is_root {
        let command = command.unwrap_or_default().replace('.', " ");
        return json!({ "error": format!("this requires root (run `sudo rcli agent {command}`)") });
    }
    match command {
        Some("password.show") => match status.secrets.get(DEVICE_PASSWORD) {
            Ok(Some(password)) => json!({ "password": password }),
            Ok(None) => json!({ "error": "no Device Password is stored" }),
            Err(e) => json!({ "error": format!("{e:#}") }),
        },
        Some("password.set") => {
            let password = request["password"].as_str().unwrap_or_default();
            if password.chars().count() < MIN_LENGTH {
                return json!({ "error": format!("the Device Password must be at least {MIN_LENGTH} characters") });
            }
            match status.identity(Some(password), None, None).await {
                Ok(_) => match status.secrets.set(DEVICE_PASSWORD, password) {
                    Ok(()) => json!({ "message": "the Device Password was changed" }),
                    Err(e) => json!({ "error": format!("the Server changed it, but the Device could not store it: {e:#}") }),
                },
                Err(e) => json!({ "error": e }),
            }
        }
        Some("id.set") => {
            let device_id = request["device_id"].as_str().unwrap_or_default();
            match status.identity(None, Some(device_id), None).await {
                Ok(id) => json!({ "device_id": id, "message": format!("the Device ID is now {id}") }),
                Err(e) => json!({ "error": e }),
            }
        }
        Some("name.set") => {
            // Empty means "unchanged" on the wire, so a blank name is refused here.
            let name = request["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() || name.len() > 255 {
                return json!({ "error": "name must be 1-255 characters" });
            }
            match status.identity(None, None, Some(name)).await {
                Ok(_) => json!({ "message": "the Device name was changed" }),
                Err(e) => json!({ "error": e }),
            }
        }
        Some("server.set") => {
            let addr = request["server"].as_str().unwrap_or_default().trim();
            let key = request["key"].as_str().unwrap_or_default().trim();
            let parts = key.strip_prefix("rk_").and_then(|rest| rest.split_once('_'));
            if addr.is_empty() || parts.is_none_or(|(fingerprint, secret)| fingerprint.len() != 64 || secret.is_empty()) {
                return json!({ "error": "give the Server Address (host:port) and a Server Key rk_<fingerprint>_<secret>" });
            }
            status.set_endpoint(ServerEndpoint { addr: addr.into(), server_key: key.into() });
            json!({ "server": addr, "message": format!("the Agent is reconnecting to {addr}") })
        }
        Some(other) => json!({ "error": format!("unknown command {other:?}") }),
        None => json!({ "error": "missing command" }),
    }
}

/// Sends one request to the Agent at `path` and returns its reply (an `error` reply is returned as `Err`).
#[cfg(unix)]
pub async fn send(path: &Path, request: Value) -> anyhow::Result<Value> {
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach the Agent at {}: {e}", path.display()))?;
    let (read, mut write) = tokio::io::split(stream);
    write.write_all(format!("{request}\n").as_bytes()).await?;
    write.flush().await?;
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let reply: Value = serde_json::from_str(&line)?;
    match reply["error"].as_str() {
        Some(e) => anyhow::bail!("{e}"),
        None => Ok(reply),
    }
}

/// Sends one command with no arguments.
#[cfg(unix)]
pub async fn request(path: &Path, command: &str) -> anyhow::Result<Value> {
    send(path, json!({ "command": command })).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_store::FileSecretStore;

    #[test]
    fn only_root_may_show_or_change_the_device() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        store.set(DEVICE_PASSWORD, "abcd2345ef").unwrap();
        let status = Status::new("123456789", ServerEndpoint { addr: "localhost:1".into(), server_key: String::new() }, Arc::new(store));

        let show = json!({ "command": "password.show" });
        let block = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(block.block_on(dispatch(&status, &show, true))["password"], "abcd2345ef");
        for command in ["password.show", "id.set", "name.set", "server.set", "password.set"] {
            let reply = block.block_on(dispatch(&status, &json!({ "command": command }), false));
            assert!(reply["error"].as_str().unwrap().contains("root"), "{command}: {reply}");
        }
        assert_eq!(block.block_on(dispatch(&status, &json!({ "command": "info" }), false))["device_id"], "123456789");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_change_is_at_least_eight_characters() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-short-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        let status = Status::new("123456789", ServerEndpoint { addr: "localhost:1".into(), server_key: String::new() }, Arc::new(store));

        let reply = tokio::runtime::Runtime::new().unwrap().block_on(dispatch(&status, &json!({ "command": "password.set", "password": "short" }), true));
        assert!(reply["error"].as_str().unwrap().contains("8 characters"), "{reply}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
