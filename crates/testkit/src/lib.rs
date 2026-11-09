//! In-process test harness: a real Server on Postgres (testcontainers), real Agents on the
//! FakePlatform, a Controller API client, the `rcli` CLI, and a manual clock.
//!
//! Tests drive the system only through the Controller API (or `rcli`) and assert on what comes back.
//! System tests live in `crates/testkit/tests/<area>.rs`, one file per area.

mod live;

use std::{future::Future, sync::Arc, time::Duration};

pub use live::{local_candidate_type, LiveViewer, TestTurn};

pub use rcli_agent::platform::{self, Button, DisplayInfo, FakePlatform, Input, SessionState, UiElement};
use rcli_agent::{Credentials, ServerEndpoint};
pub use rcli_server::clock::{Clock, ManualClock};
pub use reqwest::StatusCode;
pub use rmcp;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt},
};
use tokio::task::JoinHandle;

/// Shared secret between the Server and the TURN relay (coturn `static-auth-secret`).
pub const TURN_SECRET: &str = "test-turn-secret";

/// First-run token from the Server config; the harness uses it once to create the `admin` user.
pub const BOOTSTRAP_TOKEN: &str = "test-bootstrap-token";

pub struct TestSystem {
    pub clock: Arc<ManualClock>,
    pub server: rcli_server::Server,
    /// Controller API client authenticated as the `admin` user.
    pub api: ApiClient,
    pub admin_token: String,
    /// PKCS#8 Ed25519 key that signs Agent builds; the Server hands its public half to Devices at registration.
    pub update_signing_key: Vec<u8>,
    /// The TURN relay the Server hands out, if started with `start_with_turn`.
    pub turn: Option<TestTurn>,
    /// The running Server's config (`database_url` reaches its Postgres).
    pub config: rcli_server::Config,
    _postgres: ContainerAsync<Postgres>,
}

impl TestSystem {
    pub async fn start() -> Self {
        Self::start_on("127.0.0.1:0".parse().unwrap()).await
    }

    /// Like `start`, with a TURN relay configured as the Server's ICE server.
    pub async fn start_with_turn() -> Self {
        Self::start_inner("127.0.0.1:0".parse().unwrap(), Some(TestTurn::start(TURN_SECRET).await), Some(Self::master_key())).await
    }

    /// Like `start`, with the Controller API on a fixed address (the Dashboard's Playwright stack).
    pub async fn start_on(http_addr: std::net::SocketAddr) -> Self {
        Self::start_inner(http_addr, None, Some(Self::master_key())).await
    }

    /// Like `start`, without `RCLI_MASTER_KEY`: the vault is off and secrets are stored in plaintext.
    pub async fn start_without_master_key() -> Self {
        Self::start_inner("127.0.0.1:0".parse().unwrap(), None, None).await
    }

    /// The `RCLI_MASTER_KEY` test Servers use.
    pub fn master_key() -> rcli_server::vault::MasterKey {
        "42".repeat(32).parse().unwrap()
    }

    /// Stops the Server and starts it again on the same database with `master_key` (API on a new port).
    pub async fn restart(&mut self, master_key: Option<rcli_server::vault::MasterKey>) {
        self.config.master_key = master_key;
        self.config.http_addr = "127.0.0.1:0".parse().unwrap();
        self.server = rcli_server::start(self.config.clone(), self.clock.clone()).await.expect("restart server");
        self.api = ApiClient::new(format!("http://{}", self.server.http_addr), Some(&self.admin_token));
    }

    async fn start_inner(http_addr: std::net::SocketAddr, turn: Option<TestTurn>, master_key: Option<rcli_server::vault::MasterKey>) -> Self {
        let postgres = Postgres::default().with_tag("16-alpine").start().await.expect("start postgres container");
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let clock = Arc::new(ManualClock::default());
        let (update_signing_key, update_public_key) = rcli_protocol::update::generate_key().unwrap();
        let config = rcli_server::Config {
            database_url: format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
            http_addr,
            agent_addr: "127.0.0.1:0".parse().unwrap(),
            agent_hostnames: vec!["localhost".into()],
            bootstrap_admin_token: BOOTSTRAP_TOKEN.into(),
            master_key,
            ice_urls: turn.iter().map(|t| format!("turn:{}?transport=udp", t.addr)).collect(),
            turn_secret: turn.as_ref().map(|_| TURN_SECRET.into()),
            update_public_key: Some(update_public_key),
        };
        let server = rcli_server::start(config.clone(), clock.clone()).await.expect("start server");
        let mut sys = Self {
            clock,
            api: ApiClient::new(format!("http://{}", server.http_addr), Some(BOOTSTRAP_TOKEN)),
            server,
            admin_token: String::new(),
            update_signing_key,
            turn,
            config,
            _postgres: postgres,
        };
        sys.admin_token = sys.create_user("admin", "admin").await;
        sys.api = sys.api.with_token(Some(&sys.admin_token));
        sys
    }

    pub fn advance(&self, seconds: i64) {
        self.clock.advance(chrono::Duration::seconds(seconds));
    }

    /// The Server Key as an admin reads it (`rcli server-key show`).
    pub fn server_key(&self) -> String {
        self.server.server_key.clone()
    }

    /// `localhost:port` of the Agent port, as an Agent is given it.
    pub fn agent_addr(&self) -> String {
        format!("localhost:{}", self.server.agent_addr.port())
    }

    pub fn endpoint(&self) -> ServerEndpoint {
        ServerEndpoint { addr: self.agent_addr(), server_key: self.server_key() }
    }

    /// Runs `rcli <args>` in-process against this Server as the admin; returns stdout.
    pub async fn rcli(&self, args: &[&str]) -> anyhow::Result<String> {
        self.rcli_as(&self.admin_token, args).await
    }

    /// Runs `rcli <args>` in-process with the API token `token`; returns stdout.
    pub async fn rcli_as(&self, token: &str, args: &[&str]) -> anyhow::Result<String> {
        let base = format!("http://{}", self.server.http_addr);
        let mut argv = vec!["rcli", "--server", &base, "--token", token];
        argv.extend_from_slice(args);
        let mut out = Vec::new();
        rcli::run(argv, &mut out).await?;
        Ok(String::from_utf8(out)?)
    }

    /// Starts `rcli mcp` in-process with the API token `token` (over an in-memory pipe instead of stdio)
    /// and returns a connected MCP client. Dropping or cancelling the client ends the MCP server.
    pub async fn mcp_as(&self, token: &str) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        use rmcp::ServiceExt;
        let (client, server) = tokio::io::duplex(1 << 20);
        let (base, token) = (format!("http://{}", self.server.http_addr), token.to_owned());
        tokio::spawn(async move {
            let (read, write) = tokio::io::split(server);
            rcli::mcp::serve(base, token, read, write).await
        });
        ().serve(client).await.expect("MCP handshake")
    }

    /// Creates a user with `role` ("admin" | "operator") and returns a fresh API token for it.
    pub async fn create_user(&self, name: &str, role: &str) -> String {
        let (status, body) = self.api.post_json("/api/v1/users", &serde_json::json!({ "name": name, "role": role })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (status, body) = self.api.post(&format!("/api/v1/users/{name}/tokens")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["token"].as_str().unwrap().to_owned()
    }

    /// The current TOTP code for a base32 `secret` (as an authenticator app would show it), by the Server clock.
    pub fn totp(&self, secret: &str) -> String {
        let secret = data_encoding::BASE32_NOPAD.decode(secret.as_bytes()).expect("base32 TOTP secret");
        format!("{:06}", rcli_server::auth::totp(&secret, self.clock.now().timestamp()))
    }

    /// Registers a Device on `platform` with the Server Key (not started).
    pub async fn register(&self, platform: &FakePlatform) -> anyhow::Result<Credentials> {
        rcli_agent::register(&self.endpoint(), platform).await
    }

    /// Registers a new Device named `hostname` and starts its Agent.
    pub async fn start_agent(&self, hostname: &str) -> TestAgent {
        self.start_agent_on(FakePlatform::new(hostname)).await
    }

    /// Registers a new Device named `hostname` and starts its Agent with its local IPC socket in `dir`.
    /// The Agent's Secret Store lives in `dir` too, as on a real Device.
    pub async fn start_agent_with_socket(&self, hostname: &str, dir: &std::path::Path) -> TestAgent {
        let creds = self.register(&FakePlatform::new(hostname)).await.expect("register");
        let mut agent = TestAgent::with_secrets_dir(self.endpoint(), FakePlatform::new(hostname), creds, dir.to_path_buf());
        agent.start();
        tokio::spawn(rcli_agent::ipc::serve(dir.join("agent.sock"), agent.status.clone()));
        agent
    }

    /// A Controller API client for a password connection: Server Key + Device ID + Device Password.
    pub fn device_client(&self, device_id: &str, password: &str) -> DeviceClient {
        DeviceClient {
            api: ApiClient::new(format!("http://{}", self.server.http_addr), None),
            server_key: self.server_key(),
            device_id: device_id.to_owned(),
            password: password.to_owned(),
            controller: None,
        }
    }

    /// Runs `rcli <args>` in-process against the Agent IPC socket at `socket`; returns stdout.
    pub async fn agent_rcli(&self, socket: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
        let mut argv: Vec<String> = std::iter::once("rcli".to_owned()).chain(args.iter().map(|s| s.to_string())).collect();
        argv.push("--socket".into());
        argv.push(socket.to_string_lossy().into_owned());
        let mut out = Vec::new();
        rcli::run(argv, &mut out).await?;
        Ok(String::from_utf8(out)?)
    }

    /// Registers a new Device and starts its Agent on `platform` (keep a clone to script it).
    pub async fn start_agent_on(&self, platform: FakePlatform) -> TestAgent {
        let creds = self.register(&platform).await.expect("register");
        let mut agent = TestAgent::new(self.endpoint(), platform, creds);
        agent.start();
        agent
    }

    /// Polls the Device list until the Device with 9-digit Device ID `id` satisfies `pred`; returns it.
    pub async fn wait_device(&self, id: &str, pred: impl Fn(&Value) -> bool) -> Value {
        eventually(|| async {
            let (_, devices) = self.api.get("/api/v1/devices").await;
            devices.as_array()?.iter().find(|d| d["device_id"] == id && pred(d)).cloned()
        })
        .await
    }

    /// Polls `GET /api/v1/devices/{id}` (inventory + current resources) until `pred` holds; returns it.
    pub async fn wait_device_details(&self, id: &str, pred: impl Fn(&Value) -> bool) -> Value {
        eventually(|| async {
            let (_, device) = self.api.get(&format!("/api/v1/devices/{id}")).await;
            pred(&device).then_some(device)
        })
        .await
    }

    /// `GET /api/v1/devices/{id}/history?{query}`, asserting 200.
    pub async fn history(&self, id: &str, query: &str) -> Value {
        let (status, body) = self.api.get(&format!("/api/v1/devices/{id}/history?{query}")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Polls the Device's history (`query` as for `history`) until `pred` holds; returns it.
    pub async fn wait_history(&self, id: &str, query: &str, pred: impl Fn(&Value) -> bool) -> Value {
        eventually(|| async {
            let h = self.history(id, query).await;
            pred(&h).then_some(h)
        })
        .await
    }
}

/// A real Agent running on a FakePlatform. Dropping it disconnects.
pub struct TestAgent {
    pub endpoint: ServerEndpoint,
    pub platform: FakePlatform,
    pub creds: Credentials,
    /// The Agent's local IPC view (Device ID and connection status).
    pub status: Arc<rcli_agent::ipc::Status>,
    task: Option<JoinHandle<()>>,
    network: NetworkLink,
}

impl TestAgent {
    /// An Agent for already-enrolled credentials; call `start` to connect.
    /// It reaches `endpoint` through a link the test can cut (`cut_network`). Needs a Tokio runtime.
    pub fn new(endpoint: ServerEndpoint, platform: FakePlatform, creds: Credentials) -> Self {
        let dir = std::env::temp_dir().join(format!("rcli-test-agent-{}-{}", std::process::id(), creds.device_id));
        Self::with_secrets_dir(endpoint, platform, creds, dir)
    }

    /// An Agent that keeps its Device Password in `secrets_dir`.
    pub fn with_secrets_dir(endpoint: ServerEndpoint, platform: FakePlatform, creds: Credentials, secrets_dir: std::path::PathBuf) -> Self {
        let network = NetworkLink::new(&endpoint.addr);
        use rcli_agent::secret_store::SecretStore;
        let secrets = std::sync::Arc::new(rcli_agent::secret_store::FileSecretStore::new(secrets_dir));
        if !creds.device_password.is_empty() {
            secrets.set(rcli_agent::ipc::DEVICE_PASSWORD, &creds.device_password).expect("store the Device Password");
        }
        let status = Arc::new(rcli_agent::ipc::Status::new(&creds.device_id, &endpoint.addr, secrets));
        Self { endpoint, platform, creds, status, task: None, network }
    }

    /// The Device Password generated at registration.
    pub fn device_password(&self) -> Option<String> {
        (!self.creds.device_password.is_empty()).then(|| self.creds.device_password.clone())
    }

    /// Drops the Agent's connection and stalls new ones until `restore_network`; the Agent keeps running.
    pub fn cut_network(&self) {
        self.network.up.send_replace(false);
    }

    pub fn restore_network(&self) {
        self.network.up.send_replace(true);
    }

    pub fn device_id(&self) -> &str {
        &self.creds.device_id
    }

    pub fn start(&mut self) {
        let endpoint = ServerEndpoint { addr: self.network.addr.clone(), ..self.endpoint.clone() };
        let run = rcli::agent::serve(endpoint, self.creds.clone(), Arc::new(self.platform.clone()), Some(self.status.clone()));
        self.task = Some(tokio::spawn(run));
    }

    /// Drops the Agent's connection (like the Device losing network or powering off).
    pub fn stop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        self.stop();
        self.network.task.abort();
    }
}

/// TCP forwarder between an Agent and the Server that a test can take down and bring back.
struct NetworkLink {
    /// `localhost:port` to give the Agent (keeps the TLS server name).
    addr: String,
    up: Arc<tokio::sync::watch::Sender<bool>>,
    task: JoinHandle<()>,
}

impl NetworkLink {
    fn new(target: &str) -> Self {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();
        let addr = format!("localhost:{}", std_listener.local_addr().unwrap().port());
        let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
        let up = Arc::new(tokio::sync::watch::channel(true).0);
        let (target, link) = (target.to_owned(), up.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut agent, _)) = listener.accept().await else { continue };
                let (target, mut up) = (target.clone(), link.subscribe());
                tokio::spawn(async move {
                    // A cut link stalls new connections (no reset), so the Agent reconnects the moment it is restored.
                    if up.wait_for(|up| *up).await.is_err() {
                        return;
                    }
                    let Ok(mut server) = tokio::net::TcpStream::connect(&target).await else { return };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut agent, &mut server) => {}
                        _ = up.wait_for(|up| !*up) => {}
                    }
                });
            }
        });
        Self { addr, up, task }
    }
}

/// Whether the tests run as root (then the IPC password gate lets them through).
pub fn running_as_root() -> bool {
    #[cfg(unix)]
    return std::process::Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0");
    #[cfg(not(unix))]
    false
}

/// Retries `f` until it returns `Some`, for up to 10 seconds of real time.
pub async fn eventually<T, F, Fut>(mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = f().await {
            return v;
        }
        assert!(tokio::time::Instant::now() < deadline, "condition not met within 10s");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A Controller API client for a password connection (Server Key + Device ID + Device Password).
pub struct DeviceClient {
    api: ApiClient,
    server_key: String,
    device_id: String,
    password: String,
    /// Controller machine name reported to the Server; the fake name is used when unset.
    controller: Option<String>,
}

impl DeviceClient {
    pub fn with_server_key(mut self, key: &str) -> Self {
        self.server_key = key.to_owned();
        self
    }

    pub fn with_controller(mut self, hostname: &str) -> Self {
        self.controller = Some(hostname.to_owned());
        self
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.api.http.get(self.url(path))).await
    }

    pub async fn post(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.api.http.post(self.url(path))).await
    }

    pub async fn post_json(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        self.send(self.api.http.post(self.url(path)).json(body)).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api.base)
    }

    async fn send(&self, mut req: reqwest::RequestBuilder) -> (StatusCode, Value) {
        req = req
            .header("x-rcli-server-key", &self.server_key)
            .header("x-rcli-device", &self.device_id)
            .header("x-rcli-device-password", &self.password)
            .header("x-rcli-controller", self.controller.as_deref().unwrap_or("test-controller"));
        let resp = req.send().await.expect("controller API request");
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}

pub struct ApiClient {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base: String, token: Option<&str>) -> Self {
        Self { base, token: token.map(str::to_owned), http: reqwest::Client::new() }
    }

    pub fn with_token(&self, token: Option<&str>) -> Self {
        Self::new(self.base.clone(), token)
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.http.get(format!("{}{path}", self.base))).await
    }

    pub async fn post(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.http.post(format!("{}{path}", self.base))).await
    }

    pub async fn delete(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.http.delete(format!("{}{path}", self.base))).await
    }

    pub async fn post_json(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        self.send(self.http.post(format!("{}{path}", self.base)).json(body)).await
    }

    pub async fn patch_json(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        self.send(self.http.patch(format!("{}{path}", self.base)).json(body)).await
    }

    pub async fn put_bytes(&self, path: &str, body: Vec<u8>) -> (StatusCode, Value) {
        self.send(self.http.put(format!("{}{path}", self.base)).body(body)).await
    }

    /// Opens a streaming GET (e.g. the Control Lock `watch`) and waits for its first chunk. Drop it to disconnect.
    pub async fn watch(&self, path: &str) -> reqwest::Response {
        let mut req = self.http.get(format!("{}{path}", self.base));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let mut resp = req.send().await.expect("controller API request");
        assert_eq!(resp.status(), StatusCode::OK);
        resp.chunk().await.expect("first chunk").expect("stream ended");
        resp
    }

    async fn send(&self, mut req: reqwest::RequestBuilder) -> (StatusCode, Value) {
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.expect("controller API request");
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}
