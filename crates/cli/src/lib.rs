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
    /// Manage Enrollment Codes (admin).
    #[command(subcommand)]
    EnrollmentCode(EnrollmentCodeCommand),
    /// Manage users (admin).
    #[command(subcommand)]
    User(UserCommand),
    /// Manage per-user API tokens (admin).
    #[command(subcommand)]
    Token(TokenCommand),
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
    let api = Api {
        base: cli.server.ok_or_else(|| anyhow::anyhow!("--server or RCLI_SERVER is required"))?,
        token: cli.token.ok_or_else(|| anyhow::anyhow!("--token or RCLI_TOKEN is required"))?,
        http: reqwest::Client::new(),
    };
    let (body, human) = match cli.command {
        Command::Devices => {
            let devices = api.call(reqwest::Method::GET, "/api/v1/devices", None).await?;
            let mut text = format!("{:<36}  {:<24}  {:<7}  {}\n", "ID", "NAME", "STATUS", "LAST SEEN");
            for d in devices.as_array().into_iter().flatten() {
                text += &format!(
                    "{:<36}  {:<24}  {:<7}  {}\n",
                    str_of(&d["id"]),
                    str_of(&d["name"]),
                    str_of(&d["status"]),
                    d["last_seen_at"].as_str().unwrap_or("-"),
                );
            }
            (devices, text)
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
    };
    if cli.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&body)?)?;
    } else {
        write!(out, "{human}")?;
    }
    Ok(())
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
        let mut req = self.http.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let e = &body["error"];
            anyhow::bail!("{} ({}): {}", str_of(&e["code"]), status, str_of(&e["message"]));
        }
        Ok(body)
    }
}
