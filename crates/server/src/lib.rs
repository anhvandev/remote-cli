//! Server: Device registry, enrollment and the Controller API.
//!
//! Layout: one module per server area (`registry`, `enrollment`, ...), the Agent
//! connection in `agent_link`, and HTTP routes per area under `api/`.

pub mod agent_link;
pub mod api;
pub mod audit;
pub mod auth;
pub mod clock;
pub mod control_lock;
pub mod enrollment;
pub mod history;
pub mod metrics;
pub mod pki;
pub mod registry;
pub mod updates;

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
    /// DNS names / IPs Agents use to reach `agent_addr`; put in the server certificate.
    pub agent_hostnames: Vec<String>,
    /// First-run admin token: accepted only until an enabled admin user has a live API token.
    pub bootstrap_admin_token: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str, default: Option<&str>| -> anyhow::Result<String> {
            std::env::var(k).or_else(|_| default.map(str::to_owned).ok_or_else(|| anyhow::anyhow!("{k} is required")))
        };
        Ok(Self {
            database_url: var("DATABASE_URL", None)?,
            http_addr: var("RCLI_HTTP_ADDR", Some("0.0.0.0:8080"))?.parse()?,
            agent_addr: var("RCLI_AGENT_ADDR", Some("0.0.0.0:8443"))?.parse()?,
            agent_hostnames: var("RCLI_AGENT_HOSTNAMES", Some("localhost"))?
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect(),
            bootstrap_admin_token: var("RCLI_BOOTSTRAP_ADMIN_TOKEN", None)?,
        })
    }
}

/// Shared state handed to every area module.
pub struct AppState {
    pub db: PgPool,
    pub clock: Arc<dyn Clock>,
    pub ca: pki::Ca,
    pub config: Config,
    pub connections: registry::Connections,
    pub resources: metrics::CurrentResources,
    pub agents: agent_link::Agents,
    pub maintenance: history::Maintenance,
    /// Signalled on every rollout so live Device sessions offer the update.
    pub rollouts: tokio::sync::watch::Sender<()>,
}

/// A running Server. Dropping it stops the listeners.
pub struct Server {
    pub http_addr: SocketAddr,
    pub agent_addr: SocketAddr,
    /// CA certificate Agents must trust to reach the Server.
    pub ca_pem: String,
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
    let ca = pki::Ca::load_or_create(&db).await?;
    let ca_pem = ca.cert_pem.clone();

    let http = tokio::net::TcpListener::bind(config.http_addr).await?;
    let agents = tokio::net::TcpListener::bind(config.agent_addr).await?;
    let (http_addr, agent_addr) = (http.local_addr()?, agents.local_addr()?);

    let state = Arc::new(AppState {
        db,
        clock,
        ca,
        config,
        connections: Default::default(),
        resources: Default::default(),
        agents: Default::default(),
        maintenance: Default::default(),
        rollouts: tokio::sync::watch::Sender::new(()),
    });
    let acceptor = agent_link::acceptor(&state)?;
    let router = api::router(state.clone());
    let tasks = vec![
        tokio::spawn(async move {
            if let Err(e) = axum::serve(http, router).await {
                tracing::error!("controller API stopped: {e}");
            }
        }),
        tokio::spawn(agent_link::serve(agents, acceptor, state.clone())),
        tokio::spawn(async move {
            let mut every_minute = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                every_minute.tick().await;
                if let Err(e) = history::maintain(&state).await {
                    tracing::error!("history rollup/retention failed: {e:#}");
                }
            }
        }),
    ];
    Ok(Server { http_addr, agent_addr, ca_pem, tasks })
}
