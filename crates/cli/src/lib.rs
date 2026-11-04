//! `rcli`: a thin client over the Controller API. `run` is the whole CLI, so tests call it in-process.
//! Add a command: a `Command` variant plus a `match` arm; print human text, or the API JSON with `--json`.

use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};

use base64::Engine;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "rcli", about = "Remote CLI controller")]
struct Cli {
    /// Controller API base URL, e.g. http://localhost:8080
    #[arg(long, env = "RCLI_SERVER", global = true)]
    server: Option<String>,
    /// API token.
    #[arg(long, env = "RCLI_TOKEN", global = true, hide_env_values = true)]
    token: Option<String>,
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List Devices with their Device Status.
    Devices,
    /// One Device: inventory, current resources, processes.
    #[command(subcommand)]
    Device(DeviceCommand),
    /// Resource history of Devices.
    #[command(subcommand)]
    Metrics(MetricsCommand),
    /// Manage Enrollment Codes (admin).
    #[command(subcommand)]
    EnrollmentCode(EnrollmentCodeCommand),
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
    /// Observe a Device's screen (no Control Lock needed).
    Shot {
        device: String,
        /// Display index or `all`; default: the display last acted on.
        #[arg(long)]
        display: Option<String>,
        /// Directory to save images in.
        #[arg(long, default_value = ".")]
        out: PathBuf,
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
        /// Observation mode: shot or none.
        #[arg(long, global = true, default_value = "shot")]
        observe: String,
        /// Display to observe, index or `all`; default: the display acted on.
        #[arg(long, global = true)]
        observe_display: Option<String>,
        /// Directory to save images in.
        #[arg(long, global = true, default_value = ".")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum LockCommand {
    Acquire { device: String },
    Release { device: String },
    Status { device: String },
}

#[derive(Subcommand)]
enum ActCommand {
    Move { x: u32, y: u32 },
    Click { x: u32, y: u32 },
    DoubleClick { x: u32, y: u32 },
    RightClick { x: u32, y: u32 },
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
    Type { text: String },
    /// Press a key combination, e.g. ctrl+shift+t.
    Key { keys: String },
}

impl ActCommand {
    fn json(self, display: u32) -> Value {
        let point = |kind: &str, x, y| json!({ "type": kind, "display": display, "x": x, "y": y });
        match self {
            ActCommand::Move { x, y } => point("move", x, y),
            ActCommand::Click { x, y } => point("click", x, y),
            ActCommand::DoubleClick { x, y } => point("double_click", x, y),
            ActCommand::RightClick { x, y } => point("right_click", x, y),
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
            ActCommand::Type { text } => json!({ "type": "type", "text": text }),
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

/// Saves each image as `<out>/display-<N>.jpg`; one line per image.
fn save_observation(obs: &Value, out: &Path) -> anyhow::Result<String> {
    let mut text = String::new();
    for i in obs["images"].as_array().into_iter().flatten() {
        let display = &i["display"];
        if i["unchanged"] == true {
            text += &format!("display {display}: unchanged\n");
            continue;
        }
        let data = base64::engine::general_purpose::STANDARD.decode(str_of(&i["data"]))?;
        std::fs::create_dir_all(out)?;
        let path = out.join(format!("display-{display}.jpg"));
        std::fs::write(&path, data)?;
        text += &format!("display {display}: {}x{} {}\n", i["width"], i["height"], path.display());
    }
    if text.is_empty() {
        text = "ok\n".into();
    }
    Ok(text)
}

fn lock_text(lock: &Value) -> String {
    match lock["holder"].as_str() {
        Some(h) => format!("held by {h} since {}\n", str_of(&lock["acquired_at"])),
        None => "free\n".into(),
    }
}

#[derive(Subcommand)]
enum UpdateCommand {
    /// Create an update signing key (offline, no server needed). Agents get the printed public key via --update-key.
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
enum EnrollmentCodeCommand {
    /// Create a single-use Enrollment Code.
    Create,
}

pub async fn run<I, T>(args: I, out: &mut dyn Write) -> anyhow::Result<()>
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
    let api = Api {
        base: cli.server.ok_or_else(|| anyhow::anyhow!("--server or RCLI_SERVER is required"))?,
        token: cli.token.ok_or_else(|| anyhow::anyhow!("--token or RCLI_TOKEN is required"))?,
        http: reqwest::Client::new(),
    };
    let (body, human) = match cli.command {
        Command::Devices => {
            let devices = api.call(reqwest::Method::GET, "/api/v1/devices", None).await?;
            let mut text = format!(
                "{:<36}  {:<24}  {:<7}  {:<25}  {:<10}  {}\n",
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
                    "{:<36}  {:<24}  {:<7}  {:<25}  {:<10}  {}\n",
                    str_of(&d["id"]),
                    str_of(&d["name"]),
                    str_of(&d["status"]),
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
        Command::EnrollmentCode(EnrollmentCodeCommand::Create) => {
            let created = api.call(reqwest::Method::POST, "/api/v1/enrollment-codes", None).await?;
            let text = format!("{}\n", str_of(&created["code"]));
            (created, text)
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
            };
            let lock = api.call(method, &format!("/api/v1/devices/{device}/lock{suffix}"), None).await?;
            let text = lock_text(&lock);
            (lock, text)
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
        Command::Shot { device, display, out } => {
            let body = json!({ "actions": [], "display": display_sel(display)? });
            let obs = api.call(reqwest::Method::POST, &format!("/api/v1/devices/{device}/actions"), Some(&body)).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out)? };
            (obs, text)
        }
        Command::Act { device, action, display, observe, observe_display, out } => {
            let body = json!({
                "actions": [action.json(display)],
                "observe": observe,
                "display": display_sel(observe_display)?,
            });
            let obs = api.call(reqwest::Method::POST, &format!("/api/v1/devices/{device}/actions"), Some(&body)).await?;
            let text = if cli.json { String::new() } else { save_observation(&obs, &out)? };
            (obs, text)
        }
        Command::Update(UpdateCommand::Keygen { .. }) => unreachable!("handled before connecting"),
        Command::Update(UpdateCommand::Publish { file, version, target, key }) => {
            let build = std::fs::read(&file)?;
            let signature = rcli_protocol::update::sign(&std::fs::read(&key)?, &version, &target, &build)?;
            let url = format!("{}/api/v1/agent-builds/{version}/{target}", api.base.trim_end_matches('/'));
            let req = api.http.put(url).query(&[("signature", hex::encode(signature))]).body(build);
            let published = api.send(req).await?;
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

struct Api {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Api {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let mut req = self.http.request(method, url);
        if let Some(body) = body {
            req = req.json(body);
        }
        self.send(req).await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> anyhow::Result<Value> {
        let resp = req.bearer_auth(&self.token).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let e = &body["error"];
            anyhow::bail!("{} ({}): {}", str_of(&e["code"]), status, str_of(&e["message"]));
        }
        Ok(body)
    }
}
