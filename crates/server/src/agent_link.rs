//! The persistent Agent connection: TLS, client certificate optional (absent only to enroll).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use rcli_protocol::{
    agent::{agent_message, server_message, AgentMessage, ServerMessage},
    Conn,
};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivatePkcs8KeyDer},
    server::WebPkiClientVerifier,
    RootCertStore, ServerConfig,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
use tokio_rustls::TlsAcceptor;

use crate::{enrollment, metrics, pki, registry, updates, AppState};

/// How long a request to an Agent (e.g. the process list) may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Outbound channels to connected Agents and requests waiting for their reply.
#[derive(Default)]
pub struct Agents {
    senders: Mutex<HashMap<uuid::Uuid, mpsc::UnboundedSender<ServerMessage>>>,
    /// Keyed by (Device, request_id) so a Device can only answer its own requests.
    pending: Mutex<HashMap<(uuid::Uuid, u64), oneshot::Sender<agent_message::Msg>>>,
    next_id: AtomicU64,
}

#[derive(Debug)]
pub enum RequestError {
    NotConnected,
    Timeout,
}

impl Agents {
    /// Sends the message built from a fresh request_id and waits for the reply with that id.
    pub async fn request(
        &self,
        device: uuid::Uuid,
        build: impl FnOnce(u64) -> server_message::Msg,
    ) -> Result<agent_message::Msg, RequestError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert((device, id), tx);
        let sent = self
            .senders
            .lock()
            .unwrap()
            .get(&device)
            .is_some_and(|s| s.send(ServerMessage { msg: Some(build(id)) }).is_ok());
        let result = match sent {
            false => Err(RequestError::NotConnected),
            true => match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
                Ok(Ok(reply)) => Ok(reply),
                Ok(Err(_)) | Err(_) => Err(RequestError::Timeout),
            },
        };
        self.pending.lock().unwrap().remove(&(device, id));
        result
    }

    /// Sends a message that has no reply; false if the Device is not connected.
    pub fn send(&self, device: uuid::Uuid, msg: server_message::Msg) -> bool {
        self.senders.lock().unwrap().get(&device).is_some_and(|s| s.send(ServerMessage { msg: Some(msg) }).is_ok())
    }

    fn reply(&self, device: uuid::Uuid, request_id: u64, msg: agent_message::Msg) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&(device, request_id)) {
            let _ = tx.send(msg);
        }
    }
}

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
    match fingerprint {
        None => enroll(&mut Conn::new(tls), &state).await,
        Some(fp) => {
            let mut conn = Conn::with_max_frame(tls, rcli_protocol::update::MAX_SESSION_FRAME);
            // shortcut: unknown certificates are dropped silently; revocation (ticket 16) should send an error.
            let Some(id) = registry::find_by_cert_fingerprint(&state, &fp).await? else { return Ok(()) };
            metrics::session_started(&state, id);
            // The newest connection takes over outbound messages (an overlapping reconnect replaces the old one).
            // Registered before the Device shows Online, so Online always means requests can be sent.
            let (tx, rx) = mpsc::unbounded_channel();
            state.agents.senders.lock().unwrap().insert(id, tx.clone());
            registry::agent_connected(&state, id).await?;
            let result = session(&mut conn, rx, &state, id).await;
            {
                let mut senders = state.agents.senders.lock().unwrap();
                if senders.get(&id).is_some_and(|s| s.same_channel(&tx)) {
                    senders.remove(&id);
                }
            }
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
async fn session<T>(
    conn: &mut Conn<T>,
    mut outbound: mpsc::UnboundedReceiver<ServerMessage>,
    state: &AppState,
    device: uuid::Uuid,
) -> anyhow::Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut rollouts = state.rollouts.subscribe();
    let mut offered = None;
    loop {
        tokio::select! {
            msg = tokio::time::timeout(READ_TIMEOUT, conn.recv::<AgentMessage>()) => {
                let Some(msg) = msg?? else { return Ok(()) };
                match msg.msg {
                    Some(agent_message::Msg::ActResult(r)) => state.agents.reply(device, r.request_id, agent_message::Msg::ActResult(r)),
                    Some(agent_message::Msg::LiveAnswer(a)) => state.agents.reply(device, a.request_id, agent_message::Msg::LiveAnswer(a)),
                    Some(agent_message::Msg::Displays(d)) => state.agents.reply(device, d.request_id, agent_message::Msg::Displays(d)),
                    Some(agent_message::Msg::Inventory(inv)) => metrics::save_inventory(state, device, &inv).await?,
                    Some(agent_message::Msg::Sample(s)) => metrics::apply_sample(state, device, &s).await?,
                    Some(agent_message::Msg::Processes(p)) => {
                        state.agents.reply(device, p.request_id, agent_message::Msg::Processes(p));
                    }
                    Some(agent_message::Msg::Hello(h)) => {
                        updates::agent_hello(state, device, &h).await?;
                        offer_update(conn, state, device, &mut offered).await?;
                    }
                    Some(agent_message::Msg::UpdateFailed(f)) => updates::agent_failed(state, device, &f).await?,
                    // Later tickets dispatch more replies here.
                    _ => {}
                }
            }
            Some(msg) = outbound.recv() => conn.send(&msg).await?,
            changed = rollouts.changed() => {
                changed?;
                offer_update(conn, state, device, &mut offered).await?;
            }
        }
    }
}

/// Sends the Device its pending Agent update, once per version per session.
async fn offer_update<T>(conn: &mut Conn<T>, state: &AppState, device: uuid::Uuid, offered: &mut Option<String>) -> anyhow::Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if let Some(update) = updates::pending(state, device).await? {
        if offered.as_ref() != Some(&update.version) {
            *offered = Some(update.version.clone());
            conn.send(&ServerMessage { msg: Some(server_message::Msg::Update(update)) }).await?;
        }
    }
    Ok(())
}
