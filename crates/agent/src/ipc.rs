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

/// What `rcli agent info` reports.
pub struct Status {
    pub device_id: String,
    pub server: String,
    pub connected: AtomicBool,
}

impl Status {
    pub fn new(device_id: &str, server: &str) -> Self {
        Self { device_id: device_id.into(), server: server.into(), connected: AtomicBool::new(false) }
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
        tokio::spawn(async move {
            if let Err(e) = handle(stream, status).await {
                tracing::debug!("IPC client ended: {e}");
            }
        });
    }
}

/// Answers one request on `conn`.
async fn handle<S>(conn: S, status: Arc<Status>) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(conn);
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let request: Value = serde_json::from_str(&line)?;
    let reply = match request["command"].as_str() {
        Some("info") => status.info(),
        Some(other) => json!({ "error": format!("unknown command {other:?}") }),
        None => json!({ "error": "missing command" }),
    };
    write.write_all(format!("{reply}\n").as_bytes()).await?;
    write.flush().await?;
    Ok(())
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
