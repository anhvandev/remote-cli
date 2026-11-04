//! Agent core: enrollment and the persistent connection to the Server.

pub mod contract;
pub mod platform;

use std::{sync::Arc, time::Duration};

use platform::Platform;
use rcli_protocol::{
    agent::{
        agent_message, server_message, AgentMessage, EnrollRequest, Heartbeat, ProcessList, Resources, ServerMessage,
    },
    Conn,
};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, ServerName},
    ClientConfig, RootCertStore,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};

const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// Inventory is checked and resources are sampled this often.
const SAMPLE_EVERY: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Where the Server's Agent port is and which CA to trust for it.
#[derive(Clone, Debug)]
pub struct ServerEndpoint {
    /// `host:port`; `host` must be one of the Server's `agent_hostnames`.
    pub addr: String,
    pub ca_pem: String,
}

/// The Device's identity, obtained once by enrolling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub device_id: String,
    pub cert_pem: String,
    pub key_pem: String,
}

async fn connect(server: &ServerEndpoint, creds: Option<&Credentials>) -> anyhow::Result<Conn<TlsStream<TcpStream>>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from_pem_slice(server.ca_pem.as_bytes())?)?;
    let builder = ClientConfig::builder().with_root_certificates(roots);
    let config = match creds {
        None => builder.with_no_client_auth(),
        Some(c) => builder.with_client_auth_cert(
            vec![CertificateDer::from_pem_slice(c.cert_pem.as_bytes())?],
            PrivateKeyDer::from_pem_slice(c.key_pem.as_bytes())?,
        )?,
    };
    let host = server.addr.rsplit_once(':').map_or(server.addr.as_str(), |(h, _)| h);
    let name = ServerName::try_from(host.trim_matches(['[', ']']).to_owned())?;
    let tcp = TcpStream::connect(&server.addr).await?;
    let tls = TlsConnector::from(Arc::new(config)).connect(name, tcp).await?;
    Ok(Conn::new(tls))
}

/// Exchanges a single-use Enrollment Code for this Device's credentials.
/// The private key never leaves the Agent; only a CSR is sent.
pub async fn enroll(server: &ServerEndpoint, code: &str, platform: &dyn Platform) -> anyhow::Result<Credentials> {
    let key = rcgen::KeyPair::generate()?;
    let csr = rcgen::CertificateParams::new(vec![])?.serialize_request(&key)?.pem()?;
    let mut conn = connect(server, None).await?;
    let req = EnrollRequest { code: code.into(), csr_pem: csr, hostname: platform.hostname() };
    conn.send(&AgentMessage { msg: Some(agent_message::Msg::Enroll(req)) }).await?;
    match conn.recv::<ServerMessage>().await?.and_then(|m| m.msg) {
        Some(server_message::Msg::Enrolled(r)) => {
            Ok(Credentials { device_id: r.device_id, cert_pem: r.cert_pem, key_pem: key.serialize_pem() })
        }
        Some(server_message::Msg::Error(e)) => anyhow::bail!("enrollment rejected: {}: {}", e.code, e.message),
        Some(_) | None => anyhow::bail!("server closed the connection during enrollment"),
    }
}

/// Keeps a connection to the Server open forever, reconnecting with backoff.
pub async fn run(server: ServerEndpoint, creds: Credentials, platform: Arc<dyn Platform>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match connect(&server, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                if let Err(e) = session(conn, platform.as_ref()).await {
                    tracing::warn!("connection to server lost: {e}");
                }
            }
            Err(e) => tracing::warn!("cannot connect to server: {e}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(mut conn: Conn<TlsStream<TcpStream>>, platform: &dyn Platform) -> anyhow::Result<()> {
    let send = |msg| AgentMessage { msg: Some(msg) };
    let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
    let mut sample = tokio::time::interval(SAMPLE_EVERY);
    // What the Server has seen this session; a new session starts from nothing, so it gets everything.
    let (mut inventory, mut resources) = (None, Resources::default());
    loop {
        tokio::select! {
            _ = tick.tick() => {
                conn.send(&send(agent_message::Msg::Heartbeat(Heartbeat {}))).await?;
            }
            _ = sample.tick() => {
                // shortcut: inventory is re-read every sample; OS adapters may want change notifications instead.
                match platform.inventory() {
                    Ok(inv) if inventory.as_ref() != Some(&inv) => {
                        conn.send(&send(agent_message::Msg::Inventory(inv.clone()))).await?;
                        inventory = Some(inv);
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("cannot read inventory: {e:#}"),
                }
                match sample_resources(platform) {
                    Ok(cur) => {
                        let d = delta(&resources, &cur);
                        if d != Resources::default() {
                            conn.send(&send(agent_message::Msg::Resources(d))).await?;
                        }
                        resources = cur;
                    }
                    Err(e) => tracing::warn!("cannot sample resources: {e:#}"),
                }
            }
            msg = conn.recv::<ServerMessage>() => {
                match msg?.and_then(|m| m.msg) {
                    None => anyhow::bail!("server closed the connection"),
                    Some(server_message::Msg::ListProcesses(req)) => {
                        let reply = match platform.processes() {
                            Ok(processes) => ProcessList { request_id: req.request_id, processes, error: String::new() },
                            Err(e) => ProcessList { request_id: req.request_id, processes: vec![], error: format!("{e:#}") },
                        };
                        conn.send(&send(agent_message::Msg::Processes(reply))).await?;
                    }
                    // Later tickets handle Actions and requests from the Server here.
                    Some(_) => {}
                }
            }
        }
    }
}

fn sample_resources(platform: &dyn Platform) -> anyhow::Result<Resources> {
    use platform::SessionState as S;
    use rcli_protocol::agent::SessionState as P;
    let mut r = platform.metrics()?;
    let session = match platform.session_state()? {
        S::LoggedIn => P::Active,
        S::Locked => P::LockScreen,
        S::LoginScreen => P::LoginScreen,
    };
    r.session = Some(session.into());
    Ok(r)
}

/// The fields of `cur` that differ from `prev`.
// shortcut: a value that becomes unavailable (Some -> None) is not sent; the Server keeps the last one until reconnect.
fn delta(prev: &Resources, cur: &Resources) -> Resources {
    let mut d = Resources::default();
    macro_rules! changed {
        ($($f:ident),*) => { $( if cur.$f != prev.$f { d.$f = cur.$f.clone(); } )* };
    }
    changed!(
        cpu_percent,
        ram_used_bytes,
        disk_read_bytes_per_sec,
        disk_write_bytes_per_sec,
        net_up_bytes_per_sec,
        net_down_bytes_per_sec,
        gpu_percent,
        uptime_secs,
        logged_in_user,
        session,
        cpu_temp_celsius,
        battery_percent,
        battery_charging
    );
    d.disk_used_percent = cur
        .disk_used_percent
        .iter()
        .filter(|(k, v)| prev.disk_used_percent.get(*k) != Some(v))
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    d
}
