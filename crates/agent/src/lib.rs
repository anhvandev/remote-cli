//! Agent core: enrollment and the persistent connection to the Server.

pub mod platform;

use std::{sync::Arc, time::Duration};

use platform::Platform;
use rcli_protocol::{
    agent::{agent_message, server_message, AgentMessage, EnrollRequest, Heartbeat, ServerMessage},
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
        None => anyhow::bail!("server closed the connection during enrollment"),
    }
}

/// Keeps a connection to the Server open forever, reconnecting with backoff.
pub async fn run(server: ServerEndpoint, creds: Credentials, _platform: Arc<dyn Platform>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match connect(&server, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                if let Err(e) = session(conn).await {
                    tracing::warn!("connection to server lost: {e}");
                }
            }
            Err(e) => tracing::warn!("cannot connect to server: {e}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(mut conn: Conn<TlsStream<TcpStream>>) -> anyhow::Result<()> {
    let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                conn.send(&AgentMessage { msg: Some(agent_message::Msg::Heartbeat(Heartbeat {})) }).await?;
            }
            msg = conn.recv::<ServerMessage>() => {
                if msg?.is_none() {
                    anyhow::bail!("server closed the connection");
                }
                // Later tickets handle Actions and requests from the Server here.
            }
        }
    }
}
