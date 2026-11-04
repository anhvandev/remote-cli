//! The persistent Agent connection: TLS, client certificate optional (absent only to enroll).

use std::{sync::Arc, time::Duration};

use rcli_protocol::{
    agent::{agent_message, server_message, AgentMessage, ServerMessage},
    Conn,
};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivatePkcs8KeyDer},
    server::WebPkiClientVerifier,
    RootCertStore, ServerConfig,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::{enrollment, pki, registry, AppState};

/// No frame (heartbeats arrive every 10 s) for this long means the connection is dead.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub fn acceptor(state: &AppState) -> anyhow::Result<TlsAcceptor> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from_pem_slice(state.ca.cert_pem.as_bytes())?)?;
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots)).allow_unauthenticated().build()?;
    let leaf = state.ca.issue_server_cert(state.config.agent_hostnames.clone())?;
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![leaf.cert_der.into()], PrivatePkcs8KeyDer::from(leaf.key_der).into())?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn serve(listener: TcpListener, acceptor: TlsAcceptor, state: Arc<AppState>) {
    loop {
        let Ok((tcp, peer)) = listener.accept().await else { continue };
        let (acceptor, state) = (acceptor.clone(), state.clone());
        tokio::spawn(async move {
            if let Err(e) = handle(tcp, acceptor, state).await {
                tracing::debug!("agent connection from {peer} ended: {e}");
            }
        });
    }
}

async fn handle(tcp: TcpStream, acceptor: TlsAcceptor, state: Arc<AppState>) -> anyhow::Result<()> {
    let tls = acceptor.accept(tcp).await?;
    let fingerprint = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).map(|c| pki::fingerprint(c));
    let mut conn = Conn::new(tls);
    match fingerprint {
        None => enroll(&mut conn, &state).await,
        Some(fp) => {
            // shortcut: unknown certificates are dropped silently; revocation (ticket 16) should send an error.
            let Some(id) = registry::find_by_cert_fingerprint(&state, &fp).await? else { return Ok(()) };
            registry::agent_connected(&state, id).await?;
            let result = session(&mut conn).await;
            registry::agent_disconnected(&state, id).await?;
            result
        }
    }
}

async fn enroll<T>(conn: &mut Conn<T>, state: &AppState) -> anyhow::Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let Some(AgentMessage { msg: Some(agent_message::Msg::Enroll(req)) }) = conn.recv().await? else {
        anyhow::bail!("expected EnrollRequest");
    };
    let reply = match enrollment::enroll(state, req).await {
        Ok(r) => server_message::Msg::Enrolled(r),
        Err(e) => server_message::Msg::Error(match e {
            enrollment::EnrollError::InvalidCode => error("invalid_enrollment_code", "enrollment code is invalid or already used"),
            enrollment::EnrollError::BadRequest(m) => error("bad_request", &m),
            enrollment::EnrollError::Internal(e) => {
                tracing::error!("enrollment failed: {e:#}");
                error("internal", "internal error")
            }
        }),
    };
    conn.send(&ServerMessage { msg: Some(reply) }).await
}

fn error(code: &str, message: &str) -> rcli_protocol::agent::Error {
    rcli_protocol::agent::Error { code: code.into(), message: message.into() }
}

/// Runs until the Agent disconnects or goes silent.
async fn session<T>(conn: &mut Conn<T>) -> anyhow::Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Some(_msg) = tokio::time::timeout(READ_TIMEOUT, conn.recv::<AgentMessage>()).await?? {
        // Later tickets dispatch inventory, metrics and Action results here.
    }
    Ok(())
}
