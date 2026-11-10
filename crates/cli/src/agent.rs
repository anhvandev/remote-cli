//! `rcli agent run`: the Agent side of the binary. The Agent itself stays a library crate.

use std::{path::PathBuf, sync::Arc};

use clap::Args;
use rcli_agent::{
    ipc,
    secret_store::{FileSecretStore, SecretStore},
    settings::{AcceptMode, RootAccess},
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
    /// Root daemon: where the console session helpers (`rcli agent session`) connect to do the screen work.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[arg(long)]
    session_socket: Option<PathBuf>,
}

/// The Device's credentials from `state_dir`, or a new registration saved there. The Device Password goes to
/// `secrets`, never into `credentials.json`.
pub async fn load_or_register(
    server: &ServerEndpoint,
    platform: &dyn rcli_agent::platform::Platform,
    state_dir: &std::path::Path,
    secrets: &dyn SecretStore,
) -> anyhow::Result<Credentials> {
    let path = state_dir.join("credentials.json");
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    let creds = rcli_agent::register(server, platform).await?;
    std::fs::create_dir_all(state_dir)?;
    secrets.set(ipc::DEVICE_PASSWORD, &creds.device_password)?;
    std::fs::write(&path, serde_json::to_vec(&creds)?)?;
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(creds)
}

/// The Secret Store: the System Keychain for the root Agent on macOS, `systemd-creds` for the root service on
/// Linux (the encrypted-file fallback when it is not available); otherwise the encrypted-file fallback.
pub fn secret_store(state_dir: &std::path::Path) -> Arc<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    if crate::upgrade::is_root() {
        return Arc::new(rcli_agent::secret_store::KeychainSecretStore::system());
    }
    #[cfg(target_os = "linux")]
    if crate::upgrade::is_root() && rcli_agent::secret_store::systemd_creds_available() {
        return Arc::new(rcli_agent::secret_store::SystemdCredsSecretStore::new(state_dir));
    }
    Arc::new(FileSecretStore::new(state_dir))
}

impl Run {
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        let server = ServerEndpoint { addr: self.server, server_key: self.key };
        #[cfg(target_os = "linux")]
        let platform = Arc::new(rcli_agent::linux::LinuxPlatform::with_state_dir(self.name, &self.state_dir)?);
        #[cfg(target_os = "macos")]
        let platform = Arc::new(rcli_agent::macos::MacPlatform::new(self.name)?);
        #[cfg(windows)]
        let platform = Arc::new(rcli_agent::windows_platform::WindowsPlatform::new(self.name)?);
        // shortcut: no adapter for other OSes, so the binary runs on the FakePlatform there.
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let platform = Arc::new(rcli_agent::platform::FakePlatform::new(self.name));
        // The installed root daemon cannot reach the window server: session helpers do the screen work (ticket 24).
        #[cfg(target_os = "linux")]
        let platform: Arc<dyn rcli_agent::platform::Platform> = match &self.session_socket {
            Some(socket) => rcli_agent::linux::session::console(platform, socket, Some(&self.state_dir))?,
            None => platform,
        };
        #[cfg(target_os = "macos")]
        let platform: Arc<dyn rcli_agent::platform::Platform> = match &self.session_socket {
            Some(socket) => rcli_agent::macos_session::SessionPlatform::listen(platform, socket)?,
            None => platform,
        };
        let path = self.state_dir.join("credentials.json");
        let secrets = secret_store(&self.state_dir);
        let creds = load_or_register(&server, platform.as_ref(), &self.state_dir, secrets.as_ref()).await?;
        tracing::info!("Device {} connecting to {}", creds.device_id, server.addr);
        let status = Arc::new(ipc::Status::new(&creds.device_id, server.clone(), secrets));
        // Device Settings live beside the credentials, so `rcli agent …` changes survive a restart.
        status.set_settings_dir(self.state_dir.clone());
        // A new Device ID set at the Device must survive a restart.
        status.set_credentials_path(path);
        let socket = self.socket.unwrap_or_else(ipc::default_socket);
        // Desktop build: prompts and control events open the GUI window (ticket 14).
        #[cfg(feature = "gui")]
        let platform = crate::gui::wrap(platform, status.prompts(), socket.clone());
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
        let reply = ipc::send(&self.socket.unwrap_or_else(ipc::default_socket), &ipc::Request::Info).await?;
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
/// Allowed for any local user: the person at the Device can always stop a session on their Device.
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
        let reply = ipc::send(&socket, &ipc::Request::Disconnect { who: self.who.unwrap_or_default() }).await?;
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
        /// At least 8 characters. Reads one line from stdin when omitted, so an elevated helper
        /// never puts a password on a command line.
        #[arg(long, env = "RCLI_DEVICE_PASSWORD", hide_env_values = true)]
        password: Option<String>,
    },
}

/// `rcli agent password show|set`: the running Agent refuses non-root callers itself (peer credentials).
pub async fn run_password(command: PasswordCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let socket = socket.unwrap_or_else(ipc::default_socket);
    match command {
        PasswordCommand::Show { json: true } => {
            let reply = ipc::send(&socket, &ipc::Request::PasswordShow).await?;
            writeln!(out, "{}", serde_json::to_string_pretty(&reply)?)?;
        }
        PasswordCommand::Show { json: false } => {
            let reply = ipc::send(&socket, &ipc::Request::PasswordShow).await?;
            writeln!(out, "{}", reply["password"].as_str().unwrap_or_default())?;
        }
        PasswordCommand::Set { password } => {
            let password = match password {
                Some(p) => p,
                None => {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end_matches(['\n', '\r']).to_owned()
                }
            };
            let reply = ipc::send(&socket, &ipc::Request::PasswordSet { password }).await?;
            writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
        }
    }
    Ok(())
}

/// `rcli agent unlock-password set|clear`: the password `unlock` types (root only).
#[derive(Args)]
pub struct UnlockPassword {
    #[command(subcommand)]
    command: UnlockPasswordCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum UnlockPasswordCommand {
    /// Store (or replace) the unlock password on this Device (requires root/Administrator).
    Set {
        /// The password `unlock` types at the lock or login screen. Reads one line from stdin when omitted,
        /// so an elevated helper never puts a password on a command line.
        #[arg(long, env = "RCLI_UNLOCK_PASSWORD", hide_env_values = true)]
        password: Option<String>,
    },
    /// Forget the unlock password; `unlock` says none is set until one is stored again.
    Clear,
}

pub async fn run_unlock_password(command: UnlockPasswordCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let socket = socket.unwrap_or_else(ipc::default_socket);
    let reply = match command {
        UnlockPasswordCommand::Set { password } => {
            let password = match password {
                Some(p) => p,
                None => {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end_matches(['\n', '\r']).to_owned()
                }
            };
            ipc::send(&socket, &ipc::Request::UnlockPasswordSet { password }).await?
        }
        UnlockPasswordCommand::Clear => ipc::send(&socket, &ipc::Request::UnlockPasswordClear).await?,
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
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
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), &ipc::Request::IdSet { device_id }).await?
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
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), &ipc::Request::NameSet { name })
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
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), &ipc::Request::TerminalUserSet { user })
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
            RootCommand::Grant { who } => ipc::send(&socket, &ipc::Request::RootGrant { who }).await?,
            RootCommand::Never => ipc::send(&socket, &ipc::Request::RootSet { root: RootAccess::Never }).await?,
            RootCommand::Ask => ipc::send(&socket, &ipc::Request::RootSet { root: RootAccess::Ask }).await?,
            RootCommand::Always => ipc::send(&socket, &ipc::Request::RootSet { root: RootAccess::Always }).await?,
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
            AcceptCommand::Password => AcceptMode::Password,
            AcceptCommand::PasswordAccept => AcceptMode::PasswordAccept,
        };
        let reply = ipc::send(&socket, &ipc::Request::AcceptSet { accept }).await?;
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
            ipc::send(&socket.unwrap_or_else(ipc::default_socket), &ipc::Request::ServerSet { server: address, key }).await?
        }
    };
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}

/// `rcli agent prompt answer`: the person at the Device's answer to a waiting prompt (tickets 07/08).
/// Refusing is open to any local user; accepting needs root, so the GUI runs this command elevated.
#[derive(Args)]
pub struct Prompt {
    #[command(subcommand)]
    pub command: PromptCommand,
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
    pub socket: Option<PathBuf>,
}

#[derive(clap::Subcommand)]
pub enum PromptCommand {
    /// Accept or refuse the prompt with this id (`rcli agent prompts` lists them).
    Answer {
        id: u64,
        #[arg(value_parser = ["accept", "reject"])]
        decision: String,
    },
}

/// `rcli agent prompts`: the prompts waiting for the person at the Device (any local user).
#[derive(Args)]
pub struct Prompts {
    /// Local IPC socket of the running Agent.
    #[arg(long, env = "RCLI_AGENT_SOCKET")]
    socket: Option<PathBuf>,
    /// Machine-readable JSON output.
    #[arg(long)]
    json: bool,
}

impl Prompts {
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let reply = ipc::send(&self.socket.unwrap_or_else(ipc::default_socket), &ipc::Request::Prompts).await?;
        if self.json {
            writeln!(out, "{}", serde_json::to_string_pretty(&reply)?)?;
        } else {
            for p in reply.as_array().map(Vec::as_slice).unwrap_or_default() {
                writeln!(
                    out,
                    "{} {} ({}, {}s left)",
                    p["id"].as_u64().unwrap_or_default(),
                    p["who"].as_str().unwrap_or_default(),
                    p["kind"].as_str().unwrap_or_default(),
                    p["seconds_left"].as_u64().unwrap_or_default(),
                )?;
            }
        }
        Ok(())
    }
}

pub async fn run_prompt(command: PromptCommand, socket: Option<PathBuf>, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let PromptCommand::Answer { id, decision } = command;
    let request = ipc::Request::PromptAnswer { id, accept: decision == "accept" };
    let reply = ipc::send(&socket.unwrap_or_else(ipc::default_socket), &request).await?;
    writeln!(out, "{}", reply["message"].as_str().unwrap_or_default())?;
    Ok(())
}

/// `sudo rcli agent install`: install the Agent as a background service and register the Device.
#[derive(Args)]
pub struct Install {
    /// Server Agent port, host:port.
    #[arg(long)]
    server: String,
    /// Server Key, rk_<CA fingerprint>_<secret>.
    #[arg(long, env = "RCLI_SERVER_KEY", hide_env_values = true)]
    key: String,
    /// Device name; default: this machine's host name.
    #[arg(long)]
    name: Option<String>,
    /// The account terminal commands run as; default: the installing account (`SUDO_USER`).
    #[arg(long)]
    terminal_user: Option<String>,
    /// Always allow `--as root` (Device Setting root = always) instead of asking at the Device.
    #[arg(long)]
    allow_root: bool,
}

impl Install {
    #[cfg(target_os = "macos")]
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let name = match self.name {
            Some(name) => name,
            None => String::from_utf8(std::process::Command::new("/bin/hostname").arg("-s").output()?.stdout)?.trim().to_owned(),
        };
        let opts = crate::macos_install::Options {
            server: self.server,
            key: self.key,
            name,
            terminal_user: self.terminal_user,
            allow_root: self.allow_root,
        };
        crate::macos_install::install(opts, out).await
    }

    #[cfg(target_os = "linux")]
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        let name = match self.name {
            Some(name) => name,
            None => match std::fs::read_to_string("/proc/sys/kernel/hostname") {
                Ok(name) if !name.trim().is_empty() => name.trim().to_owned(),
                _ => String::from_utf8(std::process::Command::new("hostname").output()?.stdout)?.trim().to_owned(),
            },
        };
        let opts = crate::linux_install::Options {
            server: self.server,
            key: self.key,
            name,
            terminal_user: self.terminal_user,
            allow_root: self.allow_root,
        };
        crate::linux_install::install(opts, out).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub async fn run(self, _out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        anyhow::bail!("`rcli agent install` is not available on this OS yet")
    }
}

/// `sudo rcli agent uninstall`: remove the service and the Device's local data.
#[derive(Args)]
pub struct Uninstall {}

impl Uninstall {
    #[cfg(target_os = "macos")]
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        crate::macos_install::uninstall(out).await
    }

    #[cfg(target_os = "linux")]
    pub async fn run(self, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        crate::linux_install::uninstall(out).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub async fn run(self, _out: &mut dyn std::io::Write) -> anyhow::Result<()> {
        anyhow::bail!("`rcli agent uninstall` is not available on this OS yet")
    }
}

/// `rcli agent session` (started by the root service): does the daemon's screen work in this console session.
#[derive(Args)]
pub struct Session {
    /// The daemon's session socket.
    #[arg(long)]
    socket: PathBuf,
}

impl Session {
    #[cfg(target_os = "macos")]
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        let platform = rcli_agent::macos::MacPlatform::new("session")?;
        tokio::task::spawn_blocking(move || {
            rcli_agent::macos_session::run_helper(&self.socket, &platform, &rcli_agent::macos::on_console)
        })
        .await?
    }

    #[cfg(target_os = "linux")]
    pub async fn run(self) -> anyhow::Result<()> {
        let _ = tracing_subscriber::fmt().try_init();
        tokio::task::spawn_blocking(move || rcli_agent::linux::session::run_helper(&self.socket)).await?
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub async fn run(self) -> anyhow::Result<()> {
        anyhow::bail!("`rcli agent session` is not available on this OS yet")
    }
}
