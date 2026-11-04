//! Agent core: enrollment and the persistent connection to the Server.

pub mod contract;
mod coords;
mod executor;
mod observer;
pub mod platform;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use executor::Executor;

use platform::Platform;
use rcli_protocol::{
    agent::{agent_message, server_message, AgentMessage, DisplayList, EnrollRequest, Error, Heartbeat, ServerMessage},
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
    // Outlives sessions: the last acted-on display and what each Controller last saw survive a reconnect.
    let executor = Arc::new(Mutex::new(Executor::default()));
    let mut backoff = Duration::from_secs(1);
    loop {
        match connect(&server, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                if let Err(e) = session(conn, &platform, &executor).await {
                    tracing::warn!("connection to server lost: {e}");
                }
            }
            Err(e) => tracing::warn!("cannot connect to server: {e}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(
    mut conn: Conn<TlsStream<TcpStream>>,
    platform: &Arc<dyn Platform>,
    executor: &Arc<Mutex<Executor>>,
) -> anyhow::Result<()> {
    let send = |msg| AgentMessage { msg: Some(msg) };
    let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
    // Replies from work that runs off this loop (Actions block on the OS for up to seconds).
    let (replies, mut replies_rx) = tokio::sync::mpsc::unbounded_channel();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                conn.send(&send(agent_message::Msg::Heartbeat(Heartbeat {}))).await?;
            }
            Some(reply) = replies_rx.recv() => conn.send(&send(reply)).await?,
            msg = conn.recv::<ServerMessage>() => {
                match msg?.and_then(|m| m.msg) {
                    None => anyhow::bail!("server closed the connection"),
                    Some(server_message::Msg::Act(req)) => {
                        // The Executor's lock runs requests one at a time, in arrival order.
                        let (platform, executor, replies) = (platform.clone(), executor.clone(), replies.clone());
                        tokio::task::spawn_blocking(move || {
                            let result = executor.lock().unwrap().act(platform.as_ref(), &req);
                            let _ = replies.send(agent_message::Msg::ActResult(result));
                        });
                    }
                    Some(server_message::Msg::ListDisplays(req)) => {
                        let (displays, error) = match platform.displays() {
                            Ok(d) => (d, None),
                            Err(e) => (vec![], Some(Error { code: "platform".into(), message: e.to_string() })),
                        };
                        let reply = DisplayList { request_id: req.request_id, displays, error };
                        conn.send(&send(agent_message::Msg::Displays(reply))).await?;
                    }
                    // Later tickets handle more requests from the Server here.
                    Some(_) => {}
                }
            }
        }
    }
}
