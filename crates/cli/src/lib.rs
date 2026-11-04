//! `rcli`: a thin client over the Controller API. `run` is the whole CLI, so tests call it in-process.
//! Add a command: a `Command` variant plus a `match` arm; print human text, or the API JSON with `--json`.

use std::{ffi::OsString, io::Write};

use clap::{Parser, Subcommand};
use serde_json::Value;

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
    /// Manage Enrollment Codes (admin).
    #[command(subcommand)]
    EnrollmentCode(EnrollmentCodeCommand),
    /// Manage users (admin).
    #[command(subcommand)]
    User(UserCommand),
    /// Manage per-user API tokens (admin).
    #[command(subcommand)]
    Token(TokenCommand),
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
    };
    if cli.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&body)?)?;
    } else {
        write!(out, "{human}")?;
    }
    Ok(())
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
