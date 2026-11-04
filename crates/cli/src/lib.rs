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
    /// Manage users (admin).
    #[command(subcommand)]
    User(UserCommand),
    /// Manage per-user API tokens (admin).
    #[command(subcommand)]
    Token(TokenCommand),
    /// Signed Agent builds and rollouts (admin).
    #[command(subcommand)]
    Update(UpdateCommand),
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
