//! `rcli`: a thin client over the Controller API. `run` is the whole CLI, so tests call it in-process.
//! Add a command: a `Command` variant plus a `match` arm; print human text, or the API JSON with `--json`.

use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};

pub mod agent;
pub mod config;
pub mod media;

use base64::Engine;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub mod mcp;

#[derive(Parser)]
#[command(name = "rcli", about = "Remote CLI controller")]
struct Cli {
    /// Controller API base URL; default: the one `rcli setup` saved.
    #[arg(long, global = true)]
    server: Option<String>,
    /// API token (tests and admin scripting); default: the saved Controller configuration.
    #[arg(long, global = true, hide_env_values = true)]
    token: Option<String>,
    /// Controller config directory; default: the platform's per-user location.
    #[arg(long, global = true, hide = true)]
    config_dir: Option<String>,
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save the Server Address and Server Key (once per Controller).
    Setup {
        /// Server Address, e.g. https://server.example.com:8080.
        #[arg(long)]
        server: String,
        /// Server Key, rk_<CA fingerprint>_<secret>.
        #[arg(long, hide_env_values = true)]
        key: String,
    },
    /// Remember a Device's Device Password, so later commands take just its Device ID.
    Connect {
        device: String,
        /// The Device Password; asked for when omitted.
        #[arg(long, hide_env_values = true)]
        password: Option<String>,
    },
    /// Remove a remembered Device Password.
    Forget { device: String },
    /// List Devices with their Device Status.
    Devices,
    /// One Device: inventory, current resources, processes.
    #[command(subcommand)]
    Device(DeviceCommand),
    /// Resource history of Devices.
    #[command(subcommand)]
    Metrics(MetricsCommand),
    /// Read or replace the Server Key Agents and Controllers need to connect (admin).
    #[command(subcommand)]
    ServerKey(ServerKeyCommand),
    /// Read or set the Server Address Agents use (admin).
    #[command(subcommand)]
    ServerAddress(ServerAddressCommand),
    /// Manage users (admin).
    #[command(subcommand)]
    User(UserCommand),
    /// Manage per-user API tokens (admin).
    #[command(subcommand)]
    Token(TokenCommand),
    /// Signed Agent builds and rollouts (admin).
    #[command(subcommand)]
    Update(UpdateCommand),
    /// Control Lock: needed to send input Actions to a Device.
    #[command(subcommand)]
    Lock(LockCommand),
    /// List a Device's displays with resolution and arrangement.
    Displays { device: String },
    /// Wake an Offline Device through a Wake Relay on its LAN.
    Wake { device: String },
    /// Restart, shut down, sleep or lock the screen of a Device (all but lock respect the Control Lock).
    Power {
        device: String,
        #[arg(value_parser = ["restart", "shutdown", "sleep", "lock"])]
        action: String,
    },
    /// Observe a Device's screen (no Control Lock needed).
    Shot {
        device: String,
        /// Display index or `all`; default: the display last acted on.
        #[arg(long)]
        display: Option<String>,
        /// Number the detected Marks on the images and print the Mark table.
        #[arg(long)]
        marks: bool,
        /// Directory to save images in.
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Run a one-shot command on a Device: prints its stdout/stderr and exits with its exit code.
    Exec {
        device: String,
        /// `root`: run as root/SYSTEM (admin only). Default: the logged-in user.
        #[arg(long = "as")]
        run_as: Option<String>,
        /// Seconds before the command is killed (default 60).
        #[arg(long)]
        timeout: Option<u32>,
        /// The command, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Make the Agent type the Device's stored unlock password + Enter (needs the Control Lock).
    Unlock {
        device: String,
        /// Directory to save the Observation in.
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Store or clear a Device's unlock password in the Secrets Vault (admin).
    #[command(subcommand)]
    UnlockPassword(UnlockPasswordCommand),
    /// Interactive shell on a Device, like SSH. Exits with the remote shell's exit code.
    Shell {
        device: String,
        /// `root`: run as root/SYSTEM (admin only). Default: the logged-in user.
        #[arg(long = "as")]
        run_as: Option<String>,
    },
    /// Audit log of Actions, oldest first (admin).
    Audit {
        /// Device id or name.
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        user: Option<String>,
        /// RFC 3339 time, inclusive.
        #[arg(long)]
        since: Option<String>,
        /// RFC 3339 time, inclusive.
        #[arg(long)]
        until: Option<String>,
    },
    /// Send one Action (needs the Control Lock) and get an Observation back.
    /// Coordinates are in the Observation image space of `--display`.
    Act {
        device: String,
        #[command(subcommand)]
        action: ActCommand,
        /// Display the coordinates refer to.
        #[arg(long, global = true, default_value_t = 0)]
        display: u32,
        /// Observation mode: shot, frames, none or clip (saved as `clip.mp4`).
        #[arg(long, global = true, default_value = "shot")]
        observe: String,
        /// Display to observe, index or `all`; default: the display acted on.
        #[arg(long, global = true)]
        observe_display: Option<String>,
        /// Number the detected Marks on the Observation and print the Mark table.
        #[arg(long, global = true)]
        marks: bool,
        /// Directory to save images in.
        #[arg(long, global = true, default_value = ".")]
        out: PathBuf,
    },
    /// Serve the Controller operations as MCP tools on stdio, for AI agents (Claude Code etc.).
    Mcp,
    /// Run the Agent on this Device: one binary for Controller and Agent.
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Send an Action Batch from JSON (needs the Control Lock): a list of Actions as in the API
    /// (plus `{"type":"wait","ms":N}`), or a whole request body, which is sent as-is.
    Batch {
        device: String,
        /// JSON file; `-` reads stdin.
        file: PathBuf,
        /// Observation mode: shot, frames or none.
        #[arg(long, default_value = "shot")]
        observe: String,
        /// Display to observe, index or `all`; default: the display last acted on.
        #[arg(long)]
        display: Option<String>,
        /// Directory to save images in.
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Register on first start, then keep the Agent connected to the Server.
    Run(agent::Run),
    /// Show the Device ID and connection status (any local user).
    Info(agent::Info),
    /// The Device Password: show it (root only).
    Password {
        #[command(subcommand)]
        command: agent::PasswordCommand,
        /// Local IPC socket of the running Agent.
        #[arg(long, env = "RCLI_AGENT_SOCKET", global = true)]
        socket: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum UnlockPasswordCommand {
    /// Store (or replace) the password; it is encrypted on the Server and never shown again.
    Set {
        device: String,
        #[arg(long, env = "RCLI_UNLOCK_PASSWORD", hide_env_values = true)]
        password: String,
    },
    Clear { device: String },
    /// Whether a password is stored.
    Status { device: String },
}

#[derive(Subcommand)]
enum LockCommand {
    /// Take the Control Lock. The CLI keeps no `lock/watch` open, so the lock ends only after 10 minutes without
    /// an Action (or on release); the 60 s disconnect grace applies to watching clients (`rcli mcp`, Dashboard).
    Acquire { device: String },
    Release { device: String },
    Status { device: String },
    /// Admin: end someone else's lock (audited).
    ForceRelease { device: String },
}

#[derive(Subcommand)]
enum ActCommand {
    Move(At),
    Click(At),
    DoubleClick(At),
    RightClick(At),
    /// Drag with the left button from (x, y) to (to_x, to_y).
    Drag { x: u32, y: u32, to_x: u32, to_y: u32 },
    /// Scroll at (x, y); positive --dy scrolls down.
    Scroll {
        x: u32,
        y: u32,
        #[arg(long, default_value_t = 0, allow_negative_numbers = true)]
        dx: i32,
        #[arg(long, default_value_t = 0, allow_negative_numbers = true)]
        dy: i32,
    },
    /// Type text.
    Type {
        text: String,
        /// It is a password: masked in the audit log.
        #[arg(long)]
        secret: bool,
    },
    /// Press a key combination, e.g. ctrl+shift+t.
    Key { keys: String },
}

/// Where to point: image coordinates X Y, `--mark N --observation ID`, or `--text T`.
#[derive(clap::Args)]
struct At {
    #[arg(required_unless_present_any = ["mark", "text"])]
    x: Option<u32>,
    #[arg(required_unless_present_any = ["mark", "text"])]
    y: Option<u32>,
    /// Mark number from an Observation taken with --marks.
    #[arg(long, requires = "observation", conflicts_with_all = ["x", "y", "text"])]
    mark: Option<u32>,
    /// The Observation the Mark number comes from.
    #[arg(long)]
    observation: Option<u64>,
    /// Visible text of the target Mark.
    #[arg(long, conflicts_with_all = ["x", "y"])]
    text: Option<String>,
}

impl ActCommand {
    fn json(self, display: u32) -> Value {
        let point = |kind: &str, x, y| json!({ "type": kind, "display": display, "x": x, "y": y });
        let at = |kind: &str, at: At| match at {
            At { mark: Some(mark), observation, .. } => json!({ "type": kind, "mark": mark, "observation": observation }),
            At { text: Some(text), .. } => json!({ "type": kind, "display": display, "text": text }),
            At { x, y, .. } => json!({ "type": kind, "display": display, "x": x, "y": y }),
        };
        match self {
            ActCommand::Move(a) => at("move", a),
            ActCommand::Click(a) => at("click", a),
            ActCommand::DoubleClick(a) => at("double_click", a),
            ActCommand::RightClick(a) => at("right_click", a),
            ActCommand::Drag { x, y, to_x, to_y } => {
                let mut v = point("drag", x, y);
                v["to"] = json!({ "display": display, "x": to_x, "y": to_y });
                v
            }
            ActCommand::Scroll { x, y, dx, dy } => {
                let mut v = point("scroll", x, y);
                (v["dx"], v["dy"]) = (dx.into(), dy.into());
                v
            }
            ActCommand::Type { text, secret } => json!({ "type": "type", "text": text, "secret": secret }),
            ActCommand::Key { keys } => json!({ "type": "key", "keys": keys }),
        }
    }
}

/// `--display` value: an index or "all".
fn display_sel(s: Option<String>) -> anyhow::Result<Value> {
    Ok(match s.as_deref() {
        None => Value::Null,
        Some("all") => json!("all"),
        Some(n) => json!(n.parse::<u32>().map_err(|_| anyhow::anyhow!("--display must be an index or `all`"))?),
    })
}

/// Saves each image as `<out>/display-<N>.jpg` (frames: `display-<N>-<at_ms>ms.jpg`) and a clip as
/// `<out>/clip.mp4`; one line per file.
fn save_observation(obs: &Value, out: &Path, frames: bool) -> anyhow::Result<String> {
    let mut text = String::new();
    if let Some(data) = obs["clip"]["data"].as_str() {
        std::fs::create_dir_all(out)?;
        let path = out.join("clip.mp4");
        std::fs::write(&path, base64::engine::general_purpose::STANDARD.decode(data)?)?;
        let c = &obs["clip"];
        text += &format!("clip of display {}: {}x{} {} ms {}\n", c["display"], c["width"], c["height"], c["duration_ms"], path.display());
    }
    for i in obs["images"].as_array().into_iter().flatten() {
        let display = &i["display"];
        if i["unchanged"] == true {
            text += &format!("display {display}: unchanged\n");
            continue;
        }
        let data = base64::engine::general_purpose::STANDARD.decode(str_of(&i["data"]))?;
        std::fs::create_dir_all(out)?;
        let (name, label) = match frames {
            true => (format!("display-{display}-{}ms.jpg", i["at_ms"]), format!("display {display} +{}ms", i["at_ms"])),
            false => (format!("display-{display}.jpg"), format!("display {display}")),
        };
        let path = out.join(name);
        std::fs::write(&path, data)?;
        text += &format!("{label}: {}x{} {}\n", i["width"], i["height"], path.display());
    }
    if let Some(marks) = obs["marks"].as_array() {
        text += &format!("observation {}\n{:<4}  {:<7}  {:<12}  {:<19}  {}\n", obs["observation"], "MARK", "DISPLAY", "ROLE", "BOX", "TEXT");
        for m in marks {
            let bbox = format!("{},{} {}x{}", m["x"], m["y"], m["width"], m["height"]);
            text += &format!("{:<4}  {:<7}  {:<12}  {bbox:<19}  {}\n", m["number"].to_string(), m["display"].to_string(), str_of(&m["role"]), str_of(&m["text"]));
        }
    }
    if text.is_empty() {
        text = "ok\n".into();
    }
    Ok(text)
}

fn lock_text(lock: &Value) -> String {
    match lock["holder"].as_str() {
        Some(h) => format!("held by {h} since {}, expires {}\n", str_of(&lock["acquired_at"]), str_of(&lock["expires_at"])),
        None => "free\n".into(),
    }
}

#[derive(Subcommand)]
enum UpdateCommand {
    /// Create an update signing key (offline, no server needed). Give the public key to the Server as RCLI_UPDATE_PUBLIC_KEY.
    Keygen {
        /// Where to write the private key (PKCS#8). Keep it off the Server.
        key_file: String,
    },
    /// Sign an Agent build locally with KEY and upload it.
    Publish {
        file: String,
        #[arg(long)]
        version: String,
        /// `<os>-<arch>` the build runs on, e.g. linux-x86_64, windows-x86_64, macos-aarch64.
        #[arg(long)]
        target: String,
        /// Private key file from `rcli update keygen`.
        #[arg(long)]
        key: String,
    },
    /// Target a published version at all Devices or selected ones.
    Rollout {
        version: String,
        #[arg(long, conflicts_with = "device", required_unless_present = "device")]
        all: bool,
        /// Device id; repeat for several.
        #[arg(long)]
        device: Vec<String>,
    },
}

#[derive(Subcommand)]
enum UserCommand {
    /// Create a user.
    Create {
        name: String,
        /// admin or operator.
        #[arg(long)]
        role: String,
    },
    /// List users.
    List,
    /// Disable a user; all of their API tokens stop working.
    Disable { name: String },
    /// Set a user's Dashboard password and enroll a new TOTP secret (shown once; scan the otpauth URL).
    SetPassword {
        name: String,
        #[arg(long, env = "RCLI_NEW_PASSWORD", hide_env_values = true)]
        password: String,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    /// Issue a new API token for a user. The token is shown only once.
    Create { user: String },
    /// Revoke an API token by its id.
    Revoke { id: String },
}

#[derive(Subcommand)]
enum DeviceCommand {
    /// Inventory and current resources of a Device (id or name).
    Show { device: String },
    /// The Device's process list, fetched from its Agent now.
    Processes { device: String },
    /// Rename a Device (admin).
    Rename { device: String, name: String },
    /// Delete a Device and its history (admin).
    Delete { device: String },
    /// Revoke a Device's credential so its Agent can no longer connect (admin).
    Revoke { device: String },
    /// Move an old Device's history into a new one (e.g. after a reinstall), then delete the old one (admin).
    Merge {
        old: String,
        #[arg(long)]
        into: String,
    },
}

#[derive(Subcommand)]
enum MetricsCommand {
    /// Resource samples of a Device (id or name) over time; `--json` has every field.
    History {
        device: String,
        /// Start, RFC 3339 (e.g. 2026-01-01T00:00:00Z). Default: one hour before --to.
        #[arg(long)]
        from: Option<String>,
        /// End, RFC 3339. Default: now.
        #[arg(long)]
        to: Option<String>,
        /// 5s (raw, last 7 days) or 1m (rollups, last 90 days). Default: 5s unless --from is older than 7 days.
        #[arg(long)]
        resolution: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServerKeyCommand {
    /// Print the Server Key (rk_<CA fingerprint>_<secret>).
    Show,
    /// Replace the secret. Registered Devices stay Online; new installs and Controllers need the new key.
    Rotate,
}

#[derive(Subcommand)]
enum ServerAddressCommand {
    /// Print the Server Address.
    Show,
    /// Set the Server Address (host or host:port, comma-separated). Reissues the TLS certificate.
    Set { address: String },
}

pub async fn run<I, T>(args: I, out: &mut dyn Write) -> anyhow::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_with_input(args, None, out).await
}

/// `run` with the keystrokes for `rcli shell` taken from `input` instead of the terminal (None = stdin).
pub async fn run_with_input<I, T>(args: I, input: Option<mpsc::UnboundedReceiver<Vec<u8>>>, out: &mut dyn Write) -> anyhow::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => return Ok(write!(out, "{e}")?), // --help / --version
        Err(e) => anyhow::bail!("{e}"),
    };
    if let Command::Update(UpdateCommand::Keygen { key_file }) = &cli.command {
        let (private, public) = rcli_protocol::update::generate_key()?;
        write_private(key_file, &private)?;
        let public = hex::encode(public);
        if cli.json {
            writeln!(out, "{}", serde_json::json!({ "public_key": public }))?;
        } else {
            writeln!(out, "public key {public}")?;
        }
        return Ok(());
    }
    if let Command::Agent(command) = cli.command {
        return match command {
            AgentCommand::Run(run) => run.run().await,
            AgentCommand::Info(info) => info.run(out).await,
            AgentCommand::Password { command, socket } => agent::run_password(command, socket, out).await,
        };
    }
    let dir = cli.config_dir.clone().map_or_else(config::dir, Into::into);
    if let Command::Setup { server, key } = &cli.command {
        let text = config::setup(&dir, server, key).await?;
        let body = config::load_at(&dir)?.map(|c| config::json(&c)).unwrap_or(serde_json::Value::Null);
        return match cli.json {
            true => Ok(writeln!(out, "{}", serde_json::to_string_pretty(&body)?)?),
            false => Ok(write!(out, "{text}")?),
        };
    }
    if let Command::Connect { device, password } = &cli.command {
        let (server, key) = match (cli.server.clone(), config::load_at(&dir)?) {
            (Some(server), config) => (server, config.map(|c| c.server_key).unwrap_or_default()),
            (None, Some(config)) if !config.server_key.is_empty() => (config.server, config.server_key),
            _ => anyhow::bail!("run `rcli setup` first (no saved Server Address or Server Key)"),
        };
        let password = match password {
            Some(p) => p.clone(),
            None => prompt_password("Device Password: ")?,
        };
        let controller = config::hostname();
        config::verify(&server, &key, device, &password, &controller).await?;
        config::remember(&dir, device, &password, &controller)?;
        let text = format!("connected to {device} as {controller}\n");
        return match cli.json {
            true => Ok(writeln!(out, "{}", json!({ "device_id": device, "controller": controller }))?),
            false => Ok(write!(out, "{text}")?),
        };
    }
    if let Command::Forget { device } = &cli.command {
        config::forget(&dir, device)?;
        return match cli.json {
            true => Ok(writeln!(out, "{}", json!({ "forgotten": device }))?),
            false => Ok(writeln!(out, "forgot {device}")?),
        };
    }
    let (base, connection) = api_target(&dir, cli.server, cli.token)?;
    let api = Api { base, dir, connection, http: reqwest::Client::new() };
    let (body, human) = match cli.command {
        Command::Setup { .. } | Command::Connect { .. } | Command::Forget { .. } => unreachable!("handled before connecting"),
        Command::Devices => {
            let devices = api.call(reqwest::Method::GET, "/api/v1/devices", None).await?;
            let mut text = format!(
                "{:<9}  {:<24}  {:<7}  {:<25}  {:<10}  {}\n",
                "ID", "NAME", "STATUS", "LAST SEEN", "AGENT", "UPDATE"
            );
            for d in devices.as_array().into_iter().flatten() {
                let u = &d["update"];
                let update = if u.is_null() {
                    "-".to_owned()
                } else {
                    let error = u["error"].as_str().map(|e| format!(": {e}")).unwrap_or_default();
                    format!("{} {}{error}", str_of(&u["version"]), str_of(&u["status"]))
                };
                text += &format!(
                    "{:<9}  {:<24}  {:<7}  {:<25}  {:<10}  {}\n",
                    str_of(&d["device_id"]),
                    str_of(&d["name"]),
                    match d["wake_error"].as_str() {
                        Some(e) => format!("{} ({e})", str_of(&d["status"])),
                        None => str_of(&d["status"]).to_owned(),
                    },
                    d["last_seen_at"].as_str().unwrap_or("-"),
                    d["agent_version"].as_str().unwrap_or("-"),
                    update,
                );
            }
            (devices, text)
        }
        Command::Device(DeviceCommand::Show { device }) => {
            let d = api.call(reqwest::Method::GET, &format!("/api/v1/devices/{}", path_segment(&device)), None).await?;
            let mut text = String::new();
            flatten("", &d, &mut text);
            (d, text)
        }
        Command::Device(DeviceCommand::Processes { device }) => {
            let path = format!("/api/v1/devices/{}/processes", path_segment(&device));
            let procs = api.call(reqwest::Method::GET, &path, None).await?;
            let mut text = format!("{:>7}  {:<16}  {:>6}  {:>12}  {}\n", "PID", "USER", "CPU%", "MEM BYTES", "NAME");
            for p in procs.as_array().into_iter().flatten() {
                text += &format!(
                    "{:>7}  {:<16}  {:>6.1}  {:>12}  {}\n",
                    p["pid"],
                    str_of(&p["user"]),
                    p["cpu_percent"].as_f64().unwrap_or(0.0),
                    p["memory_bytes"],
                    str_of(&p["name"]),
                );
            }
            (procs, text)
        }
        Command::Device(DeviceCommand::Rename { device, name }) => {
            let path = format!("/api/v1/devices/{}", path_segment(&device));
            let d = api.call(reqwest::Method::PATCH, &path, Some(&serde_json::json!({ "name": name }))).await?;
            let text = format!("renamed {} to {}\n", str_of(&d["id"]), str_of(&d["name"]));
            (d, text)
        }
        Command::Device(DeviceCommand::Delete { device }) => {
            let path = format!("/api/v1/devices/{}", path_segment(&device));
            (api.call(reqwest::Method::DELETE, &path, None).await?, format!("deleted {device}\n"))
        }
        Command::Device(DeviceCommand::Revoke { device }) => {
            let path = format!("/api/v1/devices/{}/revoke", path_segment(&device));
            let d = api.call(reqwest::Method::POST, &path, None).await?;
            let text = format!("revoked {} at {}\n", str_of(&d["name"]), str_of(&d["revoked_at"]));
            (d, text)
        }
        Command::Device(DeviceCommand::Merge { old, into }) => {
            let path = format!("/api/v1/devices/{}/merge", path_segment(&into));
            let d = api.call(reqwest::Method::POST, &path, Some(&serde_json::json!({ "from": old }))).await?;
            let text = format!("merged {old} into {} ({})\n", str_of(&d["name"]), str_of(&d["device_id"]));
            (d, text)
        }
        Command::Metrics(MetricsCommand::History { device, from, to, resolution }) => {
            let query: Vec<String> = [("from", from), ("to", to), ("resolution", resolution)]
                .into_iter()
                .filter_map(|(k, v)| Some(format!("{k}={}", path_segment(&v?))))
                .collect();
            let path = format!("/api/v1/devices/{}/history?{}", path_segment(&device), query.join("&"));
            let history = api.call(reqwest::Method::GET, &path, None).await?;
            let mut text = format!(
                "{:<20}  {:>6}  {:>14}  {:>12}  {:>12}\n",
                "AT", "CPU%", "RAM USED BYTES", "NET UP B/S", "NET DOWN B/S"
            );
            for s in history["samples"].as_array().into_iter().flatten() {
                let (r, num) = (&s["resources"], |v: &Value| v.as_f64().map_or("-".into(), |n| format!("{n:.0}")));
                text += &format!(
                    "{:<20}  {:>6}  {:>14}  {:>12}  {:>12}\n",
                    str_of(&s["at"]),
                    r["cpu_percent"].as_f64().map_or("-".into(), |c| format!("{c:.1}")),
                    num(&r["ram_used_bytes"]),
                    num(&r["net_up_bytes_per_sec"]),
                    num(&r["net_down_bytes_per_sec"]),
                );
            }
            (history, text)
        }
        Command::ServerKey(ServerKeyCommand::Show) => {
            let key = api.call(reqwest::Method::GET, "/api/v1/server-key", None).await?;
            let text = format!("{}\n", str_of(&key["server_key"]));
            (key, text)
        }
        Command::ServerKey(ServerKeyCommand::Rotate) => {
            let key = api.call(reqwest::Method::POST, "/api/v1/server-settings/server-key/rotate", None).await?;
            let text = format!("{}\n", str_of(&key["server_key"]));
            (key, text)
        }
        Command::ServerAddress(ServerAddressCommand::Show) => {
            let s = api.call(reqwest::Method::GET, "/api/v1/server-settings", None).await?;
            let address = s["server_address"].clone();
            let text = format!("{}\n", str_of(&address));
            (serde_json::json!({ "server_address": address }), text)
        }
        Command::ServerAddress(ServerAddressCommand::Set { address }) => {
            let body = serde_json::json!({ "address": address });
            let s = api.call(reqwest::Method::PUT, "/api/v1/server-settings/server-address", Some(&body)).await?;
            let text = format!("server address {}\n", str_of(&s["server_address"]));
            (s, text)
        }
        Command::User(UserCommand::Create { name, role }) => {
            let body = serde_json::json!({ "name": name, "role": role });
            let user = api.call(reqwest::Method::POST, "/api/v1/users", Some(&body)).await?;
            let text = format!("created {} ({})\n", str_of(&user["name"]), str_of(&user["role"]));
            (user, text)
        }
        Command::User(UserCommand::List) => {
            let users = api.call(reqwest::Method::GET, "/api/v1/users", None).await?;
            let mut text = format!("{:<24}  {:<8}  {}\n", "NAME", "ROLE", "STATE");
            for u in users.as_array().into_iter().flatten() {
                let state = if u["disabled_at"].is_null() { "enabled" } else { "disabled" };
                text += &format!("{:<24}  {:<8}  {}\n", str_of(&u["name"]), str_of(&u["role"]), state);
            }
            (users, text)
        }
        Command::User(UserCommand::Disable { name }) => {
            let user = api.call(reqwest::Method::POST, &format!("/api/v1/users/{name}/disable"), None).await?;
            (user, format!("disabled {name}\n"))
        }
        Command::User(UserCommand::SetPassword { name, password }) => {
            let body = serde_json::json!({ "password": password });
            let set = api.call(reqwest::Method::POST, &format!("/api/v1/users/{name}/credentials"), Some(&body)).await?;
            let text = format!("totp_secret {}\notpauth_url {}\n", str_of(&set["totp_secret"]), str_of(&set["otpauth_url"]));
            (set, text)
        }
        Command::Token(TokenCommand::Create { user }) => {
            let issued = api.call(reqwest::Method::POST, &format!("/api/v1/users/{user}/tokens"), None).await?;
            let text = format!("id {}\ntoken {}\n", str_of(&issued["id"]), str_of(&issued["token"]));
            (issued, text)
        }
        Command::Token(TokenCommand::Revoke { id }) => {
            let body = api.call(reqwest::Method::POST, &format!("/api/v1/tokens/{id}/revoke"), None).await?;
            (body, format!("revoked {id}\n"))
        }
        Command::Lock(cmd) => {
            let (method, device, suffix) = match cmd {
                LockCommand::Acquire { device } => (reqwest::Method::POST, device, ""),
                LockCommand::Release { device } => (reqwest::Method::POST, device, "/release"),
                LockCommand::Status { device } => (reqwest::Method::GET, device, ""),
                LockCommand::ForceRelease { device } => (reqwest::Method::POST, device, "/force-release"),
            };
            let lock = api.call(method, &format!("/api/v1/devices/{device}/lock{suffix}"), None).await?;
            let text = lock_text(&lock);
            (lock, text)
        }
        Command::Wake { device } => {
            let path = format!("/api/v1/devices/{}/wake", path_segment(&device));
            let w = api.call(reqwest::Method::POST, &path, None).await?;
            let text = format!("waking {device} via {} ({})\n", str_of(&w["relay"]["name"]), str_of(&w["mac"]));
            (w, text)
        }
        Command::Power { device, action } => {
            let path = format!("/api/v1/devices/{}/power", path_segment(&device));
            let p = api.call(reqwest::Method::POST, &path, Some(&json!({ "action": action }))).await?;
            let text = format!("{device}: {action} sent\n");
            (p, text)
        }
        Command::Displays { device } => {
            let displays = api.call(reqwest::Method::GET, &format!("/api/v1/devices/{device}/displays"), None).await?;
            let mut text = format!("{:<5}  {:<11}  {:<5}  {:<11}  {}\n", "INDEX", "RESOLUTION", "SCALE", "POSITION", "PRIMARY");
            for d in displays.as_array().into_iter().flatten() {
                text += &format!(
                    "{:<5}  {:<11}  {:<5}  {:<11}  {}\n",
                    d["index"],
                    format!("{}x{}", d["width"], d["height"]),
                    d["scale"],
                    format!("{},{}", d["x"], d["y"]),
                    if d["primary"] == true { "yes" } else { "" },
                );
            }
            (displays, text)
        }
        Command::Shot { device, display, marks, out } => {
            let body = json!({ "actions": [], "display": display_sel(display)?, "marks": marks });
            let obs = api.observe(&device, &body).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out, false)? };
            (obs, text)
        }
        Command::Exec { device, run_as, timeout, command } => {
            let body = json!({ "command": command.join(" "), "as": run_as, "timeout_secs": timeout });
            let r = api.call(reqwest::Method::POST, &format!("/api/v1/devices/{}/exec", path_segment(&device)), Some(&body)).await?;
            if !cli.json {
                write!(out, "{}", str_of(&r["stdout"]))?;
                eprint!("{}", str_of(&r["stderr"]));
                let code = r["exit_code"].as_i64().unwrap_or(0) as i32;
                if r["timed_out"] == true || code != 0 {
                    return Err(RemoteExit { code, timed_out: r["timed_out"] == true }.into());
                }
            }
            (r, String::new())
        }
        Command::Unlock { device, out } => {
            let obs = api.call(reqwest::Method::POST, &format!("/api/v1/devices/{}/unlock", path_segment(&device)), None).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out, false)? };
            (obs, text)
        }
        Command::UnlockPassword(cmd) => {
            let (method, device, suffix, body) = match cmd {
                UnlockPasswordCommand::Set { device, password } => {
                    (reqwest::Method::POST, device, "", Some(json!({ "password": password })))
                }
                UnlockPasswordCommand::Clear { device } => (reqwest::Method::POST, device, "/clear", None),
                UnlockPasswordCommand::Status { device } => (reqwest::Method::GET, device, "", None),
            };
            let path = format!("/api/v1/devices/{}/unlock-password{suffix}", path_segment(&device));
            let r = api.call(method, &path, body.as_ref()).await?;
            let text = match r["updated_at"].as_str() {
                Some(at) => format!("unlock password set ({at})\n"),
                None => "no unlock password\n".into(),
            };
            (r, text)
        }
        Command::Shell { device, run_as } => {
            let code = shell(&api, &device, run_as, input.unwrap_or_else(stdin_input), out).await?;
            if code != 0 {
                return Err(RemoteExit { code, timed_out: false }.into());
            }
            return Ok(());
        }
        Command::Audit { device, user, since, until } => {
            let query: Vec<String> = [("device", device), ("user", user), ("since", since), ("until", until)]
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| format!("{k}={}", path_segment(&v))))
                .collect();
            let entries = api.call(reqwest::Method::GET, &format!("/api/v1/audit?{}", query.join("&")), None).await?;
            let mut text = format!("{:<30}  {:<16}  {:<24}  {:<8}  {:<16}  {}\n", "AT", "USER", "DEVICE", "ACTION", "RESULT", "PARAMS");
            for e in entries.as_array().into_iter().flatten() {
                let result = if e["result"].get("error").is_some_and(Value::is_null) { "ok".to_owned() } else { e["result"].to_string() };
                text += &format!(
                    "{:<30}  {:<16}  {:<24}  {:<8}  {:<16}  {}\n",
                    str_of(&e["at"]),
                    str_of(&e["user"]),
                    e["device"].as_str().or(e["device_id"].as_str()).unwrap_or(""),
                    str_of(&e["action"]),
                    result,
                    e["params"],
                );
            }
            (entries, text)
        }
        Command::Act { device, action, display, observe, observe_display, marks, out } => {
            let body = json!({
                "actions": [action.json(display)],
                "observe": observe,
                "display": display_sel(observe_display)?,
                "marks": marks,
            });
            let obs = api.observe(&device, &body).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out, observe == "frames")? };
            (obs, text)
        }
        Command::Batch { device, file, observe, display, out } => {
            let json = match file.to_str() {
                Some("-") => std::io::read_to_string(std::io::stdin())?,
                _ => std::fs::read_to_string(&file)?,
            };
            let body = match serde_json::from_str(&json)? {
                Value::Array(actions) => json!({ "actions": actions, "observe": observe, "display": display_sel(display)? }),
                body => body,
            };
            let frames = body["observe"] == "frames";
            let obs = api.observe(&device, &body).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out, frames)? };
            (obs, text)
        }
        Command::Mcp => return mcp::serve(api, tokio::io::stdin(), tokio::io::stdout()).await,
        Command::Agent(_) => unreachable!("handled before connecting"),
        Command::Update(UpdateCommand::Keygen { .. }) => unreachable!("handled before connecting"),
        Command::Update(UpdateCommand::Publish { file, version, target, key }) => {
            let build = std::fs::read(&file)?;
            let signature = rcli_protocol::update::sign(&std::fs::read(&key)?, &version, &target, &build)?;
            let url = format!("{}/api/v1/agent-builds/{version}/{target}", api.base.trim_end_matches('/'));
            let req = api.http.put(url).query(&[("signature", hex::encode(signature))]).body(build);
            let published = api.send(req, None).await?;
            (published, format!("published {version} for {target}\n"))
        }
        Command::Update(UpdateCommand::Rollout { version, all, device }) => {
            let body = if all { serde_json::json!({ "all": true }) } else { serde_json::json!({ "devices": device }) };
            let path = format!("/api/v1/agent-builds/{version}/rollout");
            let r = api.call(reqwest::Method::POST, &path, Some(&body)).await?;
            let text = format!("rolling out {version} to {} device(s)\n", r["devices"]);
            (r, text)
        }
    };
    if cli.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&body)?)?;
    } else {
        write!(out, "{human}")?;
    }
    Ok(())
}

/// Raw keystrokes from stdin, read on a plain thread (a blocked read must not hold up the runtime's exit).
fn stdin_input() -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buf = [0; 4096];
        while let Ok(n @ 1..) = std::io::Read::read(&mut std::io::stdin(), &mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

/// Puts the local terminal in raw mode (keys such as Ctrl-C go to the remote shell) until dropped.
struct RawMode;

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Runs an interactive shell session until the remote shell exits; returns its exit code.
async fn shell(
    api: &Api,
    device: &str,
    run_as: Option<String>,
    mut input: mpsc::UnboundedReceiver<Vec<u8>>,
    out: &mut dyn Write,
) -> anyhow::Result<i32> {
    use std::io::IsTerminal;
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let size = || if tty { crossterm::terminal::size().ok() } else { None };
    let (cols, rows) = size().unwrap_or((80, 24));
    let path = format!("/api/v1/devices/{}/shell", path_segment(device));
    let url = format!("{}{path}", api.base.trim_end_matches('/'));
    let body = json!({ "as": run_as, "cols": cols, "rows": rows });
    let mut resp = api.check(api.auth(api.http.post(url).json(&body), Some(&path))?.send().await?).await?;
    let _raw = if tty { crossterm::terminal::enable_raw_mode().map(|()| RawMode).ok() } else { None };
    let session_url = |session: &str, what: &str| format!("{}/api/v1/shell-sessions/{session}/{what}", api.base.trim_end_matches('/'));
    let (mut session, mut buf, mut last_size, mut input_open) = (String::new(), Vec::new(), size(), true);
    // shortcut: the terminal size is polled (no SIGWINCH / console events); fine for a human resizing a window.
    let mut size_poll = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {
            chunk = resp.chunk() => {
                let Some(chunk) = chunk? else { anyhow::bail!("the Server closed the shell stream") };
                buf.extend_from_slice(&chunk);
                while let Some(i) = buf.iter().position(|b| *b == b'\n') {
                    let line: Value = serde_json::from_slice(&buf[..i])?;
                    buf.drain(..=i);
                    if let Some(data) = line["output"].as_str() {
                        out.write_all(&base64::engine::general_purpose::STANDARD.decode(data)?)?;
                        out.flush()?;
                    } else if let Some(code) = line["exit_code"].as_i64() {
                        return Ok(code as i32);
                    } else if let Some(s) = line["session"].as_str() {
                        session = s.to_owned();
                    } else if !line["error"].is_null() {
                        anyhow::bail!("{}: {}", str_of(&line["error"]["code"]), str_of(&line["error"]["message"]));
                    }
                }
            }
            keys = input.recv(), if input_open && !session.is_empty() => match keys {
                Some(keys) => {
                    let path = format!("/api/v1/devices/{device}/shell");
                    api.check(api.auth(api.http.post(session_url(&session, "input")).body(keys), Some(&path))?.send().await?).await?;
                }
                None => input_open = false,
            },
            _ = size_poll.tick(), if tty && !session.is_empty() => {
                if let Some((cols, rows)) = size().filter(|s| Some(*s) != last_size) {
                    last_size = Some((cols, rows));
                    let body = json!({ "cols": cols, "rows": rows });
                    let path = format!("/api/v1/devices/{device}/shell");
                    api.check(api.auth(api.http.post(session_url(&session, "resize")).json(&body), Some(&path))?.send().await?).await?;
                }
            }
        }
    }
}

/// `rcli exec`'s remote command failed or timed out; `main` exits with `exit_code()` and prints nothing more.
#[derive(Debug)]
pub struct RemoteExit {
    pub code: i32,
    pub timed_out: bool,
}

impl RemoteExit {
    /// The remote exit code, or 124 (like `timeout(1)`) when the command timed out.
    pub fn exit_code(&self) -> i32 {
        if self.timed_out { 124 } else { self.code }
    }
}

impl std::fmt::Display for RemoteExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.timed_out {
            true => write!(f, "command timed out"),
            false => write!(f, "command exited with code {}", self.code),
        }
    }
}

impl std::error::Error for RemoteExit {}

fn write_private(path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    Ok(opts.open(path)?.write_all(bytes)?)
}

/// Human view of nested JSON: one `a.b[0].c: value` line per leaf, nulls and empties skipped.
fn flatten(prefix: &str, v: &Value, out: &mut String) {
    match v {
        Value::Null => {}
        Value::Object(m) => m
            .iter()
            .for_each(|(k, v)| flatten(&if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, v, out)),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, v)| flatten(&format!("{prefix}[{i}]"), v, out)),
        Value::String(s) if s.is_empty() => {}
        Value::String(s) => *out += &format!("{prefix}: {s}\n"),
        _ => *out += &format!("{prefix}: {v}\n"),
    }
}

fn path_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// How the CLI authenticates to the Controller API: a User session when it has one, else a password connection.
#[derive(Clone, Default)]
pub struct Connection {
    /// A User session (`rcli login`, or `--token` in tests).
    pub token: Option<String>,
    /// The saved Server Key: with a remembered Device Password this makes a password connection.
    pub server_key: Option<String>,
    /// This machine's name, shown in audit entries and as Control Lock holder.
    pub controller: String,
}

/// The Controller API target: `--server`/`--token`, else the saved `rcli setup` config.
fn api_target(dir: &std::path::Path, server: Option<String>, token: Option<String>) -> anyhow::Result<(String, Connection)> {
    let saved = config::load_at(dir)?;
    let controller = config::hostname();
    let (base, token, server_key) = match (server, token) {
        (Some(base), Some(token)) => (base, Some(token), None),
        (Some(base), None) => {
            (base, saved.as_ref().and_then(|c| c.token.clone()), saved.map(|c| c.server_key))
        }
        (None, Some(token)) => {
            let base = saved.map(|c| c.server).ok_or_else(|| anyhow::anyhow!("run `rcli setup` or pass --server"))?;
            (base, Some(token), None)
        }
        (None, None) => match saved.filter(|c| !c.server.is_empty()) {
            Some(c) => (c.server, c.token, Some(c.server_key)),
            None => anyhow::bail!("run `rcli setup` first (no saved Server Address or Server Key)"),
        },
    };
    Ok((base, Connection { token, server_key, controller }))
}

/// Asks for the Device Password without echoing it.
fn prompt_password(prompt: &str) -> anyhow::Result<String> {
    eprint!("{prompt}");
    let password = rpassword::read_password().map_err(|e| anyhow::anyhow!("cannot read the password: {e}"))?;
    Ok(password)
}

/// Controller API client (base URL + how to authenticate).
#[derive(Clone)]
pub struct Api {
    base: String,
    /// Where remembered Device Passwords are read from.
    dir: std::path::PathBuf,
    connection: Connection,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: String, token: String) -> Self {
        Self {
            base,
            dir: config::dir(),
            connection: Connection { token: Some(token), ..Default::default() },
            http: reqwest::Client::new(),
        }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let mut req = self.http.request(method, url);
        if let Some(body) = body {
            req = req.json(body);
        }
        self.send(req, Some(path)).await
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: Option<&str>) -> anyhow::Result<Value> {
        let resp = self.check(self.auth(req, path)?.send().await?).await?;
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    /// A User session when there is one, else the password connection of the Device in the request `path`.
    fn auth(&self, req: reqwest::RequestBuilder, path: Option<&str>) -> anyhow::Result<reqwest::RequestBuilder> {
        if let Some(token) = &self.connection.token {
            return Ok(req.bearer_auth(token));
        }
        // The path's Device authenticates the connection when its password is saved; otherwise any remembered
        // Device does, and the Server refuses the unrelated Device itself (`forbidden_device`).
        let path_device = path
            .and_then(|p| p.strip_prefix("/api/v1/devices/"))
            .and_then(|rest| rest.split('/').next())
            .filter(|d| !d.is_empty())
            .map(str::to_owned);
        let device = path_device
            .filter(|d| config::remembered(&self.dir, d).ok().flatten().is_some())
            .or_else(|| config::some_remembered(&self.dir))
            .ok_or_else(|| anyhow::anyhow!("no User session saved; run `rcli connect <Device ID>` or `rcli login`"))?;
        let Some(remembered) = config::remembered(&self.dir, &device)? else {
            anyhow::bail!("no Device Password saved for {device}; run `rcli connect {device}`");
        };
        let key = self.connection.server_key.as_deref().unwrap_or_default();
        Ok(req
            .header("x-rcli-server-key", key)
            .header("x-rcli-device", &device)
            .header("x-rcli-device-password", &remembered.password)
            .header("x-rcli-controller", &remembered.controller))
    }

    /// Turns an API error response into an error naming its code and message.
    async fn check(&self, resp: reqwest::Response) -> anyhow::Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() {
            let body: Value = resp.json().await.unwrap_or(Value::Null);
            let e = &body["error"];
            let step = e["step"].as_u64().map(|s| format!(" (step {s})")).unwrap_or_default();
            anyhow::bail!("{} ({}): {}{step}", str_of(&e["code"]), status, str_of(&e["message"]));
        }
        Ok(resp)
    }
}
