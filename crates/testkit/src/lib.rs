//! In-process test harness: a real Server on Postgres (testcontainers), real Agents on the
//! FakePlatform, a Controller API client, the `rcli` CLI, and a manual clock.
//!
//! Tests drive the system only through the Controller API (or `rcli`) and assert on what comes back.
//! System tests live in `crates/testkit/tests/<area>.rs`, one file per area.

use std::{future::Future, sync::Arc, time::Duration};

pub use rcli_agent::platform::{self, Button, DisplayInfo, FakePlatform, Input, SessionState};
use rcli_agent::{Credentials, ServerEndpoint};
pub use rcli_server::clock::ManualClock;
pub use reqwest::StatusCode;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt},
};
use tokio::task::JoinHandle;

/// First-run token from the Server config; the harness uses it once to create the `admin` user.
pub const BOOTSTRAP_TOKEN: &str = "test-bootstrap-token";

pub struct TestSystem {
    pub clock: Arc<ManualClock>,
    pub server: rcli_server::Server,
    /// Controller API client authenticated as the `admin` user.
    pub api: ApiClient,
    pub admin_token: String,
    /// PKCS#8 Ed25519 key that signs Agent builds; Agents from `endpoint()` trust its public half.
    pub update_signing_key: Vec<u8>,
    update_public_key: Vec<u8>,
    _postgres: ContainerAsync<Postgres>,
}

impl TestSystem {
    pub async fn start() -> Self {
        let postgres = Postgres::default().with_tag("16-alpine").start().await.expect("start postgres container");
        let port = postgres.get_host_port_ipv4(5432).await.unwrap();
        let clock = Arc::new(ManualClock::default());
        let config = rcli_server::Config {
            database_url: format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
            http_addr: "127.0.0.1:0".parse().unwrap(),
            agent_addr: "127.0.0.1:0".parse().unwrap(),
            agent_hostnames: vec!["localhost".into()],
            bootstrap_admin_token: BOOTSTRAP_TOKEN.into(),
        };
        let server = rcli_server::start(config, clock.clone()).await.expect("start server");
        let (update_signing_key, update_public_key) = rcli_protocol::update::generate_key().unwrap();
        let mut sys = Self {
            clock,
            api: ApiClient::new(format!("http://{}", server.http_addr), Some(BOOTSTRAP_TOKEN)),
            server,
            admin_token: String::new(),
            update_signing_key,
            update_public_key,
            _postgres: postgres,
        };
        sys.admin_token = sys.create_user("admin", "admin").await;
        sys.api = sys.api.with_token(Some(&sys.admin_token));
        sys
    }

    pub fn advance(&self, seconds: i64) {
        self.clock.advance(chrono::Duration::seconds(seconds));
    }

    pub fn endpoint(&self) -> ServerEndpoint {
        ServerEndpoint {
            addr: format!("localhost:{}", self.server.agent_addr.port()),
            ca_pem: self.server.ca_pem.clone(),
            update_key: Some(self.update_public_key.clone()),
        }
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

    /// Creates a user with `role` ("admin" | "operator") and returns a fresh API token for it.
    pub async fn create_user(&self, name: &str, role: &str) -> String {
        let (status, body) = self.api.post_json("/api/v1/users", &serde_json::json!({ "name": name, "role": role })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (status, body) = self.api.post(&format!("/api/v1/users/{name}/tokens")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["token"].as_str().unwrap().to_owned()
    }

    pub async fn create_enrollment_code(&self) -> String {
        let (status, body) = self.api.post("/api/v1/enrollment-codes").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["code"].as_str().unwrap().to_owned()
    }

    pub async fn enroll(&self, code: &str, platform: &FakePlatform) -> anyhow::Result<Credentials> {
        rcli_agent::enroll(&self.endpoint(), code, platform).await
    }

    /// Enrolls a new Device named `hostname` and starts its Agent.
    pub async fn start_agent(&self, hostname: &str) -> TestAgent {
        self.start_agent_on(FakePlatform::new(hostname)).await
    }

    /// Enrolls a new Device and starts its Agent on `platform` (keep a clone to script it).
    pub async fn start_agent_on(&self, platform: FakePlatform) -> TestAgent {
        let code = self.create_enrollment_code().await;
        let creds = self.enroll(&code, &platform).await.expect("enroll");
        let mut agent = TestAgent::new(self.endpoint(), platform, creds);
        agent.start();
        agent
    }

    /// Polls the Device list until the Device with `id` satisfies `pred`; returns it.
    pub async fn wait_device(&self, id: &str, pred: impl Fn(&Value) -> bool) -> Value {
        eventually(|| async {
            let (_, devices) = self.api.get("/api/v1/devices").await;
            devices.as_array()?.iter().find(|d| d["id"] == id && pred(d)).cloned()
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
}

/// A real Agent running on a FakePlatform. Dropping it disconnects.
pub struct TestAgent {
    pub endpoint: ServerEndpoint,
    pub platform: FakePlatform,
    pub creds: Credentials,
    task: Option<JoinHandle<()>>,
}

impl TestAgent {
    /// An Agent for already-enrolled credentials; call `start` to connect.
    pub fn new(endpoint: ServerEndpoint, platform: FakePlatform, creds: Credentials) -> Self {
        Self { endpoint, platform, creds, task: None }
    }

    pub fn device_id(&self) -> &str {
        &self.creds.device_id
    }

    pub fn start(&mut self) {
        let run = rcli_agent::run(self.endpoint.clone(), self.creds.clone(), Arc::new(self.platform.clone()));
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
    }
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

    pub async fn post_json(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        self.send(self.http.post(format!("{}{path}", self.base)).json(body)).await
    }

    pub async fn put_bytes(&self, path: &str, body: Vec<u8>) -> (StatusCode, Value) {
        self.send(self.http.put(format!("{}{path}", self.base)).body(body)).await
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
