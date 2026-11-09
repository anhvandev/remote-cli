//! `rcli agent run`: the Agent side of the binary. The Agent itself stays a library crate.

use std::{path::PathBuf, sync::Arc};

use clap::Args;
use rcli_agent::{platform::FakePlatform, Credentials, ServerEndpoint};

/// Keeps a registered Device connected: the Agent entry point `rcli agent run` and the test system share.
pub async fn serve(server: ServerEndpoint, creds: Credentials, platform: Arc<dyn rcli_agent::platform::Platform>) {
    rcli_agent::run(server, creds, platform).await;
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
}

impl Run {
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        let server = ServerEndpoint { addr: self.server, server_key: self.key };
        // shortcut: no OS adapter exists yet (tickets 28-35), so the binary runs on the FakePlatform.
        let platform = Arc::new(FakePlatform::new(self.name));
        let path = self.state_dir.join("credentials.json");
        let creds: Credentials = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) => {
                let creds = rcli_agent::register(&server, platform.as_ref()).await?;
                std::fs::create_dir_all(&self.state_dir)?;
                std::fs::write(&path, serde_json::to_vec(&creds)?)?;
                #[cfg(unix)]
                std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
                creds
            }
        };
        tracing::info!("Device {} connecting to {}", creds.device_id, server.addr);
        serve(server, creds, platform).await;
        Ok(())
    }
}
