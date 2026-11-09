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

use crate::{control_lock, metrics, pki, registration, registry, updates, AppState};

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
        self.request_within(device, REQUEST_TIMEOUT, build).await
    }

    /// `request` with a custom reply timeout, for Agent work that may run long (terminal commands).
    pub async fn request_within(
        &self,
        device: uuid::Uuid,
        timeout: Duration,
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
            true => match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(reply)) => Ok(reply),
                Ok(Err(_)) | Err(_) => Err(RequestError::Timeout),
            },
        };
        self.pending.lock().unwrap().remove(&(device, id));
        result
    }

    /// Tells the Device's Agent why and closes its current connection (if any).
    pub fn disconnect(&self, device: uuid::Uuid, code: &str, message: &str) {
        if let Some(tx) = self.senders.lock().unwrap().remove(&device) {
            let _ = tx.send(ServerMessage { msg: Some(server_message::Msg::Error(error(code, message))) });
        }
    }

    /// Fire-and-forget message to a connected Agent; dropped if it is not connected.
    pub fn send(&self, device: uuid::Uuid, msg: server_message::Msg) {
        if let Some(s) = self.senders.lock().unwrap().get(&device) {
            let _ = s.send(ServerMessage { msg: Some(msg) });
        }
    }

    fn reply(&self, device: uuid::Uuid, request_id: u64, msg: agent_message::Msg) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&(device, request_id)) {
            let _ = tx.send(msg);
        }
    }
}

/// No frame (heartbeats arrive every 10 s) for this long means the connection is dead.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub fn acceptor(ca: &crate::pki::Ca, hostnames: Vec<String>) -> anyhow::Result<TlsAcceptor> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from_pem_slice(ca.cert_pem.as_bytes())?)?;
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots)).allow_unauthenticated().build()?;
    let leaf = ca.issue_server_cert(hostnames)?;
    // The chain carries the CA so an Agent can check its fingerprint against the Server Key (it has no CA file).
    let chain = vec![leaf.cert_der.into(), ca.cert_der.clone().into()];
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, PrivatePkcs8KeyDer::from(leaf.key_der).into())?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn serve(listener: TcpListener, state: Arc<AppState>) {
    loop {
        let Ok((tcp, peer)) = listener.accept().await else { continue };
        let (acceptor, state) = (state.tls.read().await.clone(), state.clone());
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
        None => register(&mut Conn::new(tls), &state).await,
        Some(fp) => {
            let mut conn = Conn::with_max_frame(tls, rcli_protocol::update::MAX_SESSION_FRAME);
            // shortcut: a revoke landing between this lookup and registering the sender below misses this
            // connection until it reconnects; re-check after registering if that window ever matters.
            let Some(id) = registry::find_by_cert_fingerprint(&state, &fp).await? else {
                let msg = server_message::Msg::Error(error("unknown_device", "this credential is revoked or unknown"));
                return conn.send(&ServerMessage { msg: Some(msg) }).await;
            };
            metrics::session_started(&state, id);
            // The newest connection takes over outbound messages (an overlapping reconnect replaces the old one).
            // Registered before the Device shows Online, so Online always means requests can be sent.
            let (tx, rx) = mpsc::unbounded_channel();
            state.agents.senders.lock().unwrap().insert(id, tx.clone());
            registry::agent_connected(&state, id).await?;
            control_lock::push_indicator(&state, id).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let result = session(&mut conn, rx, &state, id).await;
            {
                let mut senders = state.agents.senders.lock().unwrap();
                if senders.get(&id).is_some_and(|s| s.same_channel(&tx)) {
                    senders.remove(&id);
                }
            }
            state.shells.device_gone(id);
            registry::agent_disconnected(&state, id).await?;
            result
        }
    }
}

async fn register<T>(conn: &mut Conn<T>, state: &AppState) -> anyhow::Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let Some(AgentMessage { msg: Some(agent_message::Msg::Register(req)) }) = conn.recv().await? else {
        anyhow::bail!("expected RegisterRequest");
    };
    let reply = match registration::register(state, req).await {
        Ok(r) => server_message::Msg::Registered(r),
        Err(e) => server_message::Msg::Error(match e {
            registration::RegisterError::InvalidKey => error("invalid_server_key", "the Server Key's secret is wrong"),
            registration::RegisterError::BadRequest(m) => error("bad_request", &m),
            registration::RegisterError::Internal(e) => {
                tracing::error!("registration failed: {e:#}");
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
                    Some(agent_message::Msg::WakeResult(w)) => state.agents.reply(device, w.request_id, agent_message::Msg::WakeResult(w)),
                    Some(agent_message::Msg::PowerResult(r)) => state.agents.reply(device, r.request_id, agent_message::Msg::PowerResult(r)),
                    Some(agent_message::Msg::LiveAnswer(a)) => state.agents.reply(device, a.request_id, agent_message::Msg::LiveAnswer(a)),
                    Some(agent_message::Msg::Displays(d)) => state.agents.reply(device, d.request_id, agent_message::Msg::Displays(d)),
                    Some(agent_message::Msg::ExecResult(r)) => state.agents.reply(device, r.request_id, agent_message::Msg::ExecResult(r)),
                    Some(agent_message::Msg::ShellOpened(r)) => state.agents.reply(device, r.request_id, agent_message::Msg::ShellOpened(r)),
                    Some(agent_message::Msg::ShellOutput(o)) => state.shells.deliver(device, &o.session.clone(), agent_message::Msg::ShellOutput(o)),
                    Some(agent_message::Msg::ShellExit(e)) => state.shells.deliver(device, &e.session.clone(), agent_message::Msg::ShellExit(e)),
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
                    Some(agent_message::Msg::IdentityUpdate(u)) => {
                        let reply = identity_update(state, device, u).await;
                        conn.send(&ServerMessage { msg: Some(server_message::Msg::IdentityUpdated(reply)) }).await?;
                    }
                    // Later tickets dispatch more replies here.
                    _ => {}
                }
            }
            Some(msg) = outbound.recv() => {
                conn.send(&msg).await?;
                // An Error is only sent by `Agents::disconnect`.
                if matches!(msg.msg, Some(server_message::Msg::Error(_))) {
                    return Ok(());
                }
            }
            changed = rollouts.changed() => {
                changed?;
                offer_update(conn, state, device, &mut offered).await?;
            }
        }
    }
}

/// A change made at the Device: checks everything first, then applies it, so a refused change changes nothing.
/// The reply carries the Device ID in effect afterwards.
async fn identity_update(state: &AppState, device: uuid::Uuid, u: rcli_protocol::agent::IdentityUpdate) -> rcli_protocol::agent::IdentityUpdated {
    let reply = |error: Option<rcli_protocol::agent::Error>, device_id: String| rcli_protocol::agent::IdentityUpdated {
        request_id: u.request_id,
        error,
        device_id,
    };
    let fail = |message: String| reply(Some(error("bad_request", &message)), String::new());
    if !u.device_password.is_empty() && u.device_password.chars().count() < rcli_protocol::password::MIN_LENGTH {
        return fail(format!("the Device Password must be at least {} characters", rcli_protocol::password::MIN_LENGTH));
    }
    let number = match sqlx::query_scalar::<_, String>("SELECT device_number FROM devices WHERE id = $1")
        .bind(device)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return fail("this Device no longer exists".into()),
        Err(e) => return fail(format!("{e:#}")),
    };
    if !u.device_id.is_empty() {
        match registry::set_device_number(state, device, &u.device_id).await {
            Ok(Ok(())) => {}
            Ok(Err(m)) => return fail(m),
            Err(e) => return fail(format!("{e:#}")),
        }
    }
    if !u.name.is_empty() {
        match registry::rename(state, device, &u.name).await {
            Ok(Ok(())) => {}
            Ok(Err(m)) => return fail(m),
            Err(e) => return fail(format!("{e:#}")),
        }
    }
    if !u.device_password.is_empty() {
        if let Err(e) = registry::set_device_password(state, device, &u.device_password).await {
            return fail(format!("{e:#}"));
        }
    }
    let number = if u.device_id.is_empty() { number } else { u.device_id };
    reply(None, number)
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
