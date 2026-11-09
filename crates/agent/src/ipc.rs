//! Local IPC: the running Agent serves `rcli agent ...` on a Unix socket (a named pipe on Windows later).
//! One JSON request per connection, one JSON reply, newline-terminated; only `tokio::io` traits are used, so a
//! pipe transport can reuse `handle`.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::secret_store::SecretStore;

/// The Device Password's name in the Secret Store.
pub const DEVICE_PASSWORD: &str = "device_password";

/// What the IPC clients see of the running Agent.
pub struct Status {
    pub device_id: String,
    pub server: String,
    pub connected: AtomicBool,
    pub secrets: Arc<dyn SecretStore>,
}

impl Status {
    pub fn new(device_id: &str, server: &str, secrets: Arc<dyn SecretStore>) -> Self {
        Self { device_id: device_id.into(), server: server.into(), connected: AtomicBool::new(false), secrets }
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    fn info(&self) -> Value {
        json!({
            "device_id": self.device_id,
            "server": self.server,
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let status = status.clone();
        // Only root may read the Device Password; `info` is for any local user.
        let is_root = stream.peer_cred().is_ok_and(|c| c.uid() == 0);
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
    let reply = dispatch(&status, &request, is_root);
    write.write_all(format!("{reply}\n").as_bytes()).await?;
    write.flush().await?;
    Ok(())
}

/// The reply to one request. Reading the Device Password is root-only; everything else is for any local user.
fn dispatch(status: &Status, request: &Value, is_root: bool) -> Value {
    match request["command"].as_str() {
        Some("info") => status.info(),
        Some("password.show") if is_root => match status.secrets.get(DEVICE_PASSWORD) {
            Ok(Some(password)) => json!({ "password": password }),
            Ok(None) => json!({ "error": "no Device Password is stored" }),
            Err(e) => json!({ "error": format!("{e:#}") }),
        },
        Some("password.show") => json!({ "error": "showing the Device Password requires root (run `sudo rcli agent password show`)" }),
        Some(other) => json!({ "error": format!("unknown command {other:?}") }),
        None => json!({ "error": "missing command" }),
    }
}

/// Sends one request to the Agent at `path` and returns its reply.
#[cfg(unix)]
pub async fn request(path: &Path, command: &str) -> anyhow::Result<Value> {
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach the Agent at {}: {e}", path.display()))?;
    let (read, mut write) = tokio::io::split(stream);
    write.write_all(format!("{{\"command\":\"{command}\"}}\n").as_bytes()).await?;
    write.flush().await?;
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let reply: Value = serde_json::from_str(&line)?;
    match reply["error"].as_str() {
        Some(e) => anyhow::bail!("{e}"),
        None => Ok(reply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_store::FileSecretStore;

    #[test]
    fn only_root_may_show_the_device_password() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        store.set(DEVICE_PASSWORD, "abcd2345ef").unwrap();
        let status = Status::new("123456789", "localhost:1", Arc::new(store));

        let show = json!({ "command": "password.show" });
        assert_eq!(dispatch(&status, &show, true)["password"], "abcd2345ef");
        assert!(dispatch(&status, &show, false)["error"].as_str().unwrap().contains("root"));
        assert_eq!(dispatch(&status, &json!({ "command": "info" }), false)["device_id"], "123456789");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
