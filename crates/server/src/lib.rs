//! Server: Device registry, registration and the Controller API.
//!
//! Layout: one module per server area (`registry`, `registration`, ...), the Agent
//! connection in `agent_link`, and HTTP routes per area under `api/`.

pub mod agent_link;
pub mod api;
pub mod audit;
pub mod auth;
pub mod clock;
pub mod control_lock;
pub mod history;
pub mod metrics;
pub mod pki;
pub mod registration;
pub mod registry;
pub mod server_key;
pub mod settings;
pub mod shell;
pub mod storage;
pub mod updates;
pub mod wake;
pub mod master_key;

use std::{net::SocketAddr, sync::Arc};

use clock::Clock;
use sqlx::PgPool;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    /// Controller API (HTTP).
    pub http_addr: SocketAddr,
    /// Agent connections (TLS, mTLS once enrolled).
    pub agent_addr: SocketAddr,
    /// Seeds the Server Address on first start; after that the stored setting wins (Admins edit it).
    pub agent_hostnames: Vec<String>,
    /// First-run admin token: accepted only until an enabled admin user has a live session.
    pub bootstrap_admin_token: String,
    /// Seals the CA private key and TOTP secrets (`RCLI_MASTER_KEY`); None = they are stored in plaintext.
    pub master_key: Option<master_key::MasterKey>,
    /// STUN/TURN URLs handed to both WebRTC peers, e.g. `turn:vps.example.com:3478`. Empty = direct paths only.
    pub ice_urls: Vec<String>,
    /// coturn `static-auth-secret`: the Server mints short-lived TURN credentials from it.
    pub turn_secret: Option<String>,
    /// Reverse proxies in front of the Controller API (`RCLI_TRUSTED_PROXIES`): from these peers the Controller's
    /// IP is read from `X-Forwarded-For`, so the Device Password limiter and audit see the real client.
    pub trusted_proxies: Vec<std::net::IpAddr>,
    /// The file store holding Agent builds and install packages (`RCLI_S3_*`).
    pub s3: storage::S3Config,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str, default: Option<&str>| -> anyhow::Result<String> {
            std::env::var(k).or_else(|_| default.map(str::to_owned).ok_or_else(|| anyhow::anyhow!("{k} is required")))
        };
        let s3 = ["RCLI_S3_ENDPOINT", "RCLI_S3_BUCKET", "RCLI_S3_ACCESS_KEY", "RCLI_S3_SECRET_KEY"]
            .map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).ok_or(k));
        let missing: Vec<&str> = s3.iter().filter_map(|v| v.as_ref().err().copied()).collect();
        anyhow::ensure!(missing.is_empty(), "the file storage settings are required; missing: {}", missing.join(", "));
        let [endpoint, bucket, access_key, secret_key] = s3.map(Result::unwrap);
        Ok(Self {
            s3: storage::S3Config { endpoint, bucket, access_key, secret_key },
            database_url: var("DATABASE_URL", None)?,
            http_addr: var("RCLI_HTTP_ADDR", Some("0.0.0.0:8080"))?.parse()?,
            agent_addr: var("RCLI_AGENT_ADDR", Some("0.0.0.0:8443"))?.parse()?,
            agent_hostnames: var("RCLI_AGENT_HOSTNAMES", Some("localhost"))?
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect(),
            bootstrap_admin_token: var("RCLI_BOOTSTRAP_ADMIN_TOKEN", None)?,
            master_key: std::env::var("RCLI_MASTER_KEY").ok().filter(|k| !k.is_empty()).map(|k| k.parse()).transpose()?,
            ice_urls: var("RCLI_ICE_URLS", Some(""))?.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            turn_secret: std::env::var("RCLI_TURN_SECRET").ok().filter(|s| !s.is_empty()),
            trusted_proxies: var("RCLI_TRUSTED_PROXIES", Some(""))?
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().map_err(|e| anyhow::anyhow!("RCLI_TRUSTED_PROXIES: {s}: {e}")))
                .collect::<anyhow::Result<_>>()?,
        })
    }
}

/// Shared state handed to every area module.
pub struct AppState {
    pub db: PgPool,
    pub storage: storage::Storage,
    pub clock: Arc<dyn Clock>,
    pub ca: pki::Ca,
    /// The Server Key and Server Address, editable by Admins.
    pub settings: settings::Settings,
    pub config: Config,
    /// Rebuilt when the Server Address changes, so new connections get the new certificate.
    pub tls: tokio::sync::RwLock<tokio_rustls::TlsAcceptor>,
    pub connections: registry::Connections,
    pub resources: metrics::CurrentResources,
    pub agents: agent_link::Agents,
    pub maintenance: history::Maintenance,
    pub shells: shell::Shells,
    pub lock_watchers: control_lock::Watchers,
}

/// A running Server. Dropping it stops the listeners.
pub struct Server {
    pub http_addr: SocketAddr,
    pub agent_addr: SocketAddr,
    /// CA certificate Agents must trust to reach the Server.
    pub ca_pem: String,
    /// `rk_<CA fingerprint>_<secret>`.
    pub server_key: String,
    /// The running Server's state; the testkit reaches connected Agents through it.
    pub state: Arc<AppState>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.tasks.iter().for_each(JoinHandle::abort);
    }
}

pub async fn start(config: Config, clock: Arc<dyn Clock>) -> anyhow::Result<Server> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = sqlx::postgres::PgPoolOptions::new().max_connections(10).connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    let ca = pki::Ca::load_or_create(&db, config.master_key.as_ref()).await?;
    let settings = settings::Settings::load(&db, &ca, &config.agent_hostnames.join(",")).await?;
    match &config.master_key {
        Some(key) => auth::seal_totp_secrets(&db, key).await?,
        None => tracing::warn!("no RCLI_MASTER_KEY: the CA private key and TOTP secrets are stored unencrypted"),
    }
    let ca_pem = ca.cert_pem.clone();
    let storage = storage::Storage::connect(&config.s3).await?;

    let http = tokio::net::TcpListener::bind(config.http_addr).await?;
    let agents = tokio::net::TcpListener::bind(config.agent_addr).await?;
    let (http_addr, agent_addr) = (http.local_addr()?, agents.local_addr()?);
    let acceptor = agent_link::acceptor(&ca, settings::hostnames(&settings.address().await))?;
    let server_key = settings.key().await;

    let state = Arc::new(AppState {
        db,
        storage,
        clock,
        ca,
        settings,
        config,
        tls: tokio::sync::RwLock::new(acceptor),
        connections: Default::default(),
        resources: Default::default(),
        agents: Default::default(),
        maintenance: Default::default(),
        shells: Default::default(),
        lock_watchers: Default::default(),
    });
    let router = api::router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let tasks = vec![
        tokio::spawn(async move {
            if let Err(e) = axum::serve(http, router).await {
                tracing::error!("controller API stopped: {e}");
            }
        }),
        tokio::spawn(agent_link::serve(agents, state.clone())),
        tokio::spawn({
            let state = state.clone();
            async move {
                // Expired locks are also ended lazily on every read; this hides the Device indicator on time.
                let mut every_second = tokio::time::interval(std::time::Duration::from_secs(1));
                loop {
                    every_second.tick().await;
                    if let Err(e) = control_lock::expire(&state).await {
                        tracing::error!("Control Lock expiry failed: {e:?}");
                    }
                }
            }
        }),
        tokio::spawn({
            let state = state.clone();
            async move {
                let mut every_minute = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    every_minute.tick().await;
                    if let Err(e) = history::maintain(&state).await {
                        tracing::error!("history rollup/retention failed: {e:#}");
                    }
                }
            }
        }),
    ];
    Ok(Server { http_addr, agent_addr, ca_pem, server_key, state, tasks })
}
