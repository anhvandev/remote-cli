//! `rcli agent run`: the Agent side of the binary. The Agent itself stays a library crate.

use std::{path::PathBuf, sync::Arc};

use clap::Args;
use rcli_agent::{platform::FakePlatform, Credentials, ServerEndpoint};

/// Keeps an enrolled Device connected: the Agent entry point `rcli agent run` and the test system share.
pub async fn serve(server: ServerEndpoint, creds: Credentials, platform: Arc<dyn rcli_agent::platform::Platform>) {
    rcli_agent::run(server, creds, platform).await;
}

#[derive(Args)]
pub struct Run {
    /// Server Agent port, host:port.
    #[arg(long, env = "RCLI_AGENT_SERVER")]
    server: String,
    /// Server CA certificate (PEM), from GET /api/v1/server-ca.
    #[arg(long, env = "RCLI_AGENT_CA")]
    ca: PathBuf,
    /// Where the Device credentials are kept.
    #[arg(long, env = "RCLI_AGENT_STATE_DIR")]
    state_dir: PathBuf,
    /// Enrollment Code; needed only on first start.
    #[arg(long, requires = "ca_fingerprint")]
    enroll: Option<String>,
    /// SHA-256 fingerprint of the Server CA, printed with the Enrollment Code; enrolling fails if `--ca` differs.
    #[arg(long, env = "RCLI_AGENT_CA_FINGERPRINT")]
    ca_fingerprint: Option<String>,
    /// Public key (hex) Agent updates must be signed with, from `rcli update keygen`. Without it updates are rejected.
    #[arg(long, env = "RCLI_AGENT_UPDATE_KEY")]
    update_key: Option<String>,
    /// Device name reported at enrollment.
    #[arg(long)]
    name: String,
}

impl Run {
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        let server = ServerEndpoint {
            addr: self.server,
            ca_pem: std::fs::read_to_string(&self.ca)?,
            update_key: self.update_key.map(hex::decode).transpose()?,
        };
        // shortcut: no OS adapter exists yet (tickets 28-35), so the binary runs on the FakePlatform.
        let platform = Arc::new(FakePlatform::new(self.name));
        let path = self.state_dir.join("credentials.json");
        let creds: Credentials = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) => {
                let code = self.enroll.ok_or_else(|| anyhow::anyhow!("not enrolled yet: pass --enroll <code>"))?;
                let pin = self.ca_fingerprint.unwrap_or_default();
                let creds = rcli_agent::enroll(&server, &code, &pin, platform.as_ref()).await?;
                std::fs::create_dir_all(&self.state_dir)?;
                std::fs::write(&path, serde_json::to_vec(&creds)?)?;
                #[cfg(unix)]
                std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
                creds
            }
        };
        tracing::info!("device {} connecting to {}", creds.device_id, server.addr);
        serve(server, creds, platform).await;
        Ok(())
    }
}
