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
    /// Manage Enrollment Codes (admin).
    #[command(subcommand)]
    EnrollmentCode(EnrollmentCodeCommand),
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
            let devices = api.call(reqwest::Method::GET, "/api/v1/devices").await?;
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
            let created = api.call(reqwest::Method::POST, "/api/v1/enrollment-codes").await?;
            let text = format!("{}\n", str_of(&created["code"]));
            (created, text)
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
    async fn call(&self, method: reqwest::Method, path: &str) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let resp = self.http.request(method, url).bearer_auth(&self.token).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let e = &body["error"];
            anyhow::bail!("{} ({}): {}", str_of(&e["code"]), status, str_of(&e["message"]));
        }
        Ok(body)
    }
}
