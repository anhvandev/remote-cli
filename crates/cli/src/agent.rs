//! `rcli agent run`: the Agent side of the binary. The Agent itself stays a library crate.

use std::{path::PathBuf, sync::Arc};

use clap::Args;
use rcli_agent::{
    ipc,
    platform::FakePlatform,
    secret_store::{FileSecretStore, SecretStore},
    Credentials, ServerEndpoint,
};

/// Keeps a registered Device connected: the Agent entry point `rcli agent run` and the test system share.
pub async fn serve(
    server: ServerEndpoint,
    creds: Credentials,
    platform: Arc<dyn rcli_agent::platform::Platform>,
    status: Option<Arc<ipc::Status>>,
) {
    rcli_agent::run(server, creds, platform, status).await;
}

#[derive(Args)]
pub struct Run {
    /// Server Agent port, host:port.
    #[arg(long, env = "RCLI_AGENT_SERVER")]
    server: String,
    /// Server Key, rk_<CA fingerprint>_<secret>.
    #[arg(long, env = "RCLI_SERVER_KEY", hide_env_values = true)]
    key: String,
    /// Where the Device credentials are kept.
    #[arg(long, env = "RCLI_AGENT_STATE_DIR")]
    state_dir: PathBuf,
    /// Device name reported at registration.
    #[arg(long)]
    name: String,
    /// Local IPC socket `rcli agent ...` clients use.
    #[arg(long, env = "RCLI_AGENT_SOCKET")]
    socket: Option<PathBuf>,
}

impl Run {
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        let server = ServerEndpoint { addr: self.server, server_key: self.key };
        // shortcut: no OS adapter exists yet (tickets 28-35), so the binary runs on the FakePlatform.
        let platform = Arc::new(FakePlatform::new(self.name));
        let path = self.state_dir.join("credentials.json");
        // shortcut: the encrypted-file fallback only; tickets 16/22/24 add DPAPI, Keychain and systemd-creds here.
        let secrets = FileSecretStore::new(&self.state_dir);
        let creds: Credentials = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) => {
                let creds = rcli_agent::register(&server, platform.as_ref()).await?;
                std::fs::create_dir_all(&self.state_dir)?;
                // The password never goes into credentials.json; it lives only in the Secret Store.
                secrets.set(ipc::DEVICE_PASSWORD, &creds.device_password)?;
                std::fs::write(&path, serde_json::to_vec(&creds)?)?;
                #[cfg(unix)]
                std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
                creds
            }
        };
        tracing::info!("Device {} connecting to {}", creds.device_id, server.addr);
        let status = Arc::new(ipc::Status::new(&creds.device_id, &server.addr, Arc::new(secrets)));
        let socket = self.socket.unwrap_or_else(ipc::default_socket);
        let serve_status = status.clone();
        tokio::spawn(async move {
            if let Err(e) = ipc::serve(socket, serve_status).await {
                tracing::error!("local IPC stopped: {e:#}");
            }
        });
        serve(server, creds, platform, Some(status)).await;
        Ok(())
    }
}

/// `rcli agent info`: the Device ID and connection status, readable by any local user.
#[derive(Args)]
pub struct Info {
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET")]
    socket: Option<PathBuf>,
    /// Machine-readable JSON output.
    #[arg(long)]
    json: bool,
}

impl Info {
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let reply = ipc::request(&self.socket.unwrap_or_else(ipc::default_socket), "info").await?;
        if self.json {
            writeln!(out, "{}", serde_json::to_string_pretty(&reply)?)?;
        } else {
            writeln!(
                out,
                "device id {} ({})\nserver {}",
                reply["device_id"].as_str().unwrap_or_default(),
                if reply["connected"] == true { "connected" } else { "disconnected" },
                reply["server"].as_str().unwrap_or_default(),
            )?;
        }
        Ok(())
    }
}

/// `rcli agent password ...`: show the Device Password (root only).
#[derive(Args)]
pub struct Password {
    #[command(subcommand)]
    command: PasswordCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum PasswordCommand {
    /// Show the Device Password (requires root/Administrator).
    Show {
        /// Machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
}

/// `rcli agent password show`: the running Agent refuses non-root callers itself (peer credentials).
pub async fn run_password(command: PasswordCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let reply = ipc::request(&socket.unwrap_or_else(ipc::default_socket), "password.show").await?;
    match command {
        PasswordCommand::Show { json: true } => writeln!(out, "{}", serde_json::to_string_pretty(&reply)?)?,
        PasswordCommand::Show { json: false } => writeln!(out, "{}", reply["password"].as_str().unwrap_or_default())?,
    }
    Ok(())
}
