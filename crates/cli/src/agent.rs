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
        let status = Arc::new(ipc::Status::new(&creds.device_id, server.clone(), Arc::new(secrets)));
        // Device Settings live beside the credentials, so `rcli agent …` changes survive a restart.
        status.set_settings_dir(self.state_dir.clone());
        // A new Device ID set at the Device must survive a restart.
        status.set_credentials_path(path);
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
            let controllers = reply["controllers"].as_array().map(Vec::as_slice).unwrap_or_default();
            if !controllers.is_empty() {
                writeln!(out, "controlled by:")?;
                for c in controllers {
                    writeln!(out, "  {}", c.as_str().unwrap_or_default())?;
                }
            }
        }
        Ok(())
    }
}

/// `rcli agent disconnect [<who>]`: end a Controller's Live View and shells and release its Control Lock.
/// Allowed for any local user: the person at the Device can always stop a session on their machine.
#[derive(Args)]
pub struct Disconnect {
    /// The Controller as shown at the Device, e.g. `alice` or `dev-laptop (10.0.0.5)`. Omit to disconnect all.
    who: Option<String>,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET")]
    socket: Option<PathBuf>,
}

impl Disconnect {
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let socket = self.socket.unwrap_or_else(ipc::default_socket);
        let mut body = serde_json::json!({ "command": "disconnect" });
        if let Some(who) = &self.who {
            body["who"] = who.as_str().into();
        }
        let reply = ipc::send(&socket, body).await?;
        writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
        Ok(())
    }
}

/// `rcli agent password ...`: show or change the Device Password (root only).
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
    /// Replace the Device Password (requires root/Administrator); open sessions keep working.
    Set {
        /// At least 8 characters.
        #[arg(long, env = "RCLI_DEVICE_PASSWORD", hide_env_values = true)]
        password: String,
    },
}

/// `rcli agent password show|set`: the running Agent refuses non-root callers itself (peer credentials).
pub async fn run_password(command: PasswordCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let socket = socket.unwrap_or_else(ipc::default_socket);
    match command {
        PasswordCommand::Show { json: true } => {
            let reply = ipc::request(&socket, "password.show").await?;
            writeln!(out, "{}", serde_json::to_string_pretty(&reply)?)?;
        }
        PasswordCommand::Show { json: false } => {
            let reply = ipc::request(&socket, "password.show").await?;
            writeln!(out, "{}", reply["password"].as_str().unwrap_or_default())?;
        }
        PasswordCommand::Set { password } => {
            let reply = ipc::send(&socket, serde_json::json!({ "command": "password.set", "password": password })).await?;
            writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
        }
    }
    Ok(())
}

/// `rcli agent id ...`: the Device ID (root only).
#[derive(Args)]
pub struct Id {
    #[command(subcommand)]
    command: IdCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum IdCommand {
    /// Replace the Device ID with a free 9-digit one (requires root/Administrator).
    Set { device_id: String },
}

pub async fn run_id(command: IdCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let reply = match command {
        IdCommand::Set { device_id } => {
            ipc::send(
                &socket.unwrap_or_else(ipc::default_socket),
                serde_json::json!({ "command": "id.set", "device_id": device_id }),
            )
            .await?
        }
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}

/// `rcli agent name ...`: the Device name (root only).
#[derive(Args)]
pub struct Name {
    #[command(subcommand)]
    command: NameCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum NameCommand {
    /// Rename the Device (requires root/Administrator).
    Set { name: String },
}

pub async fn run_name(command: NameCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let reply = match command {
        NameCommand::Set { name } => {
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), serde_json::json!({ "command": "name.set", "name": name }))
                .await?
        }
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}

/// `rcli agent terminal-user ...`: the account terminal commands and shells run as (root only).
#[derive(Args)]
pub struct TerminalUser {
    #[command(subcommand)]
    command: TerminalUserCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum TerminalUserCommand {
    /// Run terminal commands and shells as this account (requires root/Administrator).
    Set { user: String },
}

pub async fn run_terminal_user(command: TerminalUserCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let reply = match command {
        TerminalUserCommand::Set { user } => {
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), serde_json::json!({ "command": "terminal-user.set", "user": user }))
                .await?
        }
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}

/// `rcli agent root never|ask|always`: whether `--as root` is allowed (root only); `root grant` is for the person
/// at the Device (the GUI), allowing root to one Controller for the current session.
#[derive(Args)]
pub struct Root {
    #[command(subcommand)]
    command: RootCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum RootCommand {
    /// Root is never allowed, whoever asks.
    Never,
    /// A person at the Device decides each time (the default).
    Ask,
    /// Root is always allowed for `--as root`.
    Always,
    /// Allow root to `who` for the current session only, without changing the setting.
    Grant {
        /// The Controller as shown at the Device, e.g. `alice` or `dev-laptop (10.0.0.5)`.
        who: String,
    },
}

impl Root {
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let socket = self.socket.unwrap_or_else(ipc::default_socket);
        let reply = match self.command {
            RootCommand::Grant { who } => {
                ipc::send(&socket, serde_json::json!({ "command": "root.grant", "who": who })).await?
            }
            RootCommand::Never | RootCommand::Ask | RootCommand::Always => {
                let root = match self.command {
                    RootCommand::Never => "never",
                    RootCommand::Ask => "ask",
                    RootCommand::Always => "always",
                    RootCommand::Grant { .. } => unreachable!(),
                };
                ipc::send(&socket, serde_json::json!({ "command": "root.set", "root": root })).await?
            }
        };
        writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
        Ok(())
    }
}

/// `rcli agent accept ...`: whether a person at the Device must accept each new connection (root only).
#[derive(Args)]
pub struct Accept {
    #[command(subcommand)]
    command: AcceptCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum AcceptCommand {
    /// A Controller with the Device Password connects at once.
    Password,
    /// A person at the Device accepts each new connection (rejected after 30 s).
    #[command(name = "password+accept")]
    PasswordAccept,
}

impl Accept {
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let socket = self.socket.unwrap_or_else(ipc::default_socket);
        let accept = match self.command {
            AcceptCommand::Password => "password",
            AcceptCommand::PasswordAccept => "password+accept",
        };
        let reply = ipc::send(&socket, serde_json::json!({ "command": "accept.set", "accept": accept })).await?;
        writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
        Ok(())
    }
}

/// `rcli agent server ...`: which Server the Agent connects to (root only).
#[derive(Args)]
pub struct Server {
    #[command(subcommand)]
    command: ServerCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum ServerCommand {
    /// Point the Agent at another Server; it reconnects at once (requires root/Administrator).
    Set {
        /// host:port of the Server's Agent port.
        address: String,
        /// rk_<CA fingerprint>_<secret>.
        #[arg(long, env = "RCLI_SERVER_KEY", hide_env_values = true)]
        key: String,
    },
}

pub async fn run_server(command: ServerCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let reply = match command {
        ServerCommand::Set { address, key } => {
            ipc::send(
                &socket.unwrap_or_else(ipc::default_socket),
                serde_json::json!({ "command": "server.set", "server": address, "key": key }),
            )
            .await?
        }
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}
