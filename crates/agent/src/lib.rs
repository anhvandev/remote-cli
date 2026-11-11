//! Agent core: registration and the persistent connection to the Server.

pub mod backlog;
pub mod contract;
mod clip;
mod coords;
mod executor;
pub mod ipc;
#[cfg(target_os = "linux")]
pub mod linux;
mod live;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(unix)]
pub mod macos_session;
mod marks;
mod media;
mod observer;
pub mod platform;
pub mod secret_store;
pub mod session;
pub mod settings;
#[cfg(windows)]
pub mod windows_platform;
#[cfg(windows)]
pub mod windows_session;

use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};

use executor::Executor;

use backlog::Backlog;
use platform::Platform;
use rcli_protocol::{
    agent::{
        agent_message, server_message, AgentHello, AgentMessage, AgentUpdate, ConnectDecision, DisconnectController,
        DisplayList, Error, ExecRequest, ExecResult, Heartbeat, IdentityUpdate, IdentityUpdated, LiveAnswer,
        PowerResult, ProcessList, RegisterRequest, ResourceSample, Resources, ServerMessage, ShellExit, ShellOpen,
        ShellOpened, ShellOutput, UnregisterRequest, UpdateFailed, WakeResult,
    },
    server_key::ServerKey,
    Conn,
};
use rustls::{
    client::WebPkiServerVerifier,
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, ServerName},
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};

const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// Inventory is checked and resources are sampled this often (each sample is a history point).
const SAMPLE_EVERY: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Where the Server's Agent port is and the Server Key that pins its CA.
#[derive(Clone, Debug)]
pub struct ServerAccess {
    /// `host:port`; `host` must be one of the Server's `agent_hostnames`.
    pub addr: String,
    /// `rk_<CA fingerprint>_<secret>`.
    pub server_key: String,
}

/// `<os>-<arch>` of this Agent binary; Agent builds are signed for one target.
pub fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The Device's identity, obtained once by registering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    /// 9-digit Device ID.
    pub device_id: String,
    pub cert_pem: String,
    pub key_pem: String,
    /// DER of the Server's CA that the Server Key's fingerprint pinned at registration.
    pub ca_der: Vec<u8>,
    /// Raw Ed25519 public key pinned at registration; empty rejects every update.
    pub update_key: Vec<u8>,
    /// The Device Password returned at registration. Never written to `credentials.json`: the Agent keeps it in its
    /// Secret Store and the Server only its hash.
    #[serde(default, skip_serializing)]
    pub device_password: String,
}

fn parse_server_key(key: &str) -> anyhow::Result<ServerKey> {
    ServerKey::parse(key).ok_or_else(|| anyhow::anyhow!("the Server Key must look like rk_<fingerprint>_<secret>"))
}

/// A TLS verifier that accepts only a chain containing the CA named by `fingerprint` (hex SHA-256 of its DER),
/// then checks the Server certificate against that CA and the connection's hostname.
#[derive(Debug)]
struct PinnedCa {
    fingerprint: String,
    provider: std::sync::Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for PinnedCa {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // The Server sends [leaf, CA]; the pinned CA may also arrive as an intermediate.
        let ca = std::iter::once(end_entity)
            .chain(intermediates.iter())
            .find(|c| hex::encode(sha256(c)) == self.fingerprint)
            .ok_or_else(|| rustls::Error::General("the Server's CA fingerprint does not match the Server Key".into()))?;
        let mut roots = RootCertStore::empty();
        roots.add(ca.clone()).map_err(|e| rustls::Error::General(format!("bad Server CA: {e}")))?;
        let verifier = WebPkiServerVerifier::builder_with_provider(roots.into(), self.provider.clone())
            .build()
            .map_err(|e| rustls::Error::General(format!("cannot verify the Server: {e}")))?;
        verifier.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn sha256(der: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(der).to_vec()
}

/// Hex SHA-256 of a CA certificate (PEM).
pub fn ca_fingerprint(ca_pem: &str) -> anyhow::Result<String> {
    Ok(hex::encode(sha256(&CertificateDer::from_pem_slice(ca_pem.as_bytes())?)))
}

fn config(server: &ServerAccess, creds: Option<&Credentials>) -> anyhow::Result<ClientConfig> {
    config_pinned(&parse_server_key(&server.server_key)?.fingerprint, creds)
}

/// `config` with the pinned CA fingerprint given directly: an uninstall may not have the Server Key any more, but
/// it still has the CA the Device pinned at registration.
fn config_pinned(fingerprint: &str, creds: Option<&Credentials>) -> anyhow::Result<ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no crypto provider"))?;
    let builder = ClientConfig::builder().dangerous().with_custom_certificate_verifier(std::sync::Arc::new(PinnedCa {
        fingerprint: fingerprint.to_owned(),
        provider,
    }));
    Ok(match creds {
        None => builder.with_no_client_auth(),
        Some(c) => builder.with_client_auth_cert(
            vec![CertificateDer::from_pem_slice(c.cert_pem.as_bytes())?],
            PrivateKeyDer::from_pem_slice(c.key_pem.as_bytes())?,
        )?,
    })
}

async fn connect(server: &ServerAccess, creds: Option<&Credentials>) -> anyhow::Result<Conn<TlsStream<TcpStream>>> {
    let config = config(server, creds)?;
    connect_with_config(&server.addr, config).await
}

async fn connect_with_config(addr: &str, config: ClientConfig) -> anyhow::Result<Conn<TlsStream<TcpStream>>> {
    let host = addr.rsplit_once(':').map_or(addr, |(h, _)| h);
    let name = ServerName::try_from(host.trim_matches(['[', ']']).to_owned())?;
    let tcp = TcpStream::connect(addr).await?;
    let tls = TlsConnector::from(Arc::new(config)).connect(name, tcp).await?;
    Ok(Conn::with_max_frame(tls, rcli_protocol::update::MAX_SESSION_FRAME))
}

/// Registers this Device with the Server Key: checks the Server's CA against the key's fingerprint, presents its
/// secret, and gets back this Device's mTLS certificate, 9-digit Device ID and update key to pin.
/// The private key never leaves the Agent; only a CSR is sent.
pub async fn register(server: &ServerAccess, platform: &dyn Platform) -> anyhow::Result<Credentials> {
    let server_key = parse_server_key(&server.server_key)?;
    let key = rcgen::KeyPair::generate()?;
    let csr = rcgen::CertificateParams::new(vec![])?.serialize_request(&key)?.pem()?;
    let mut conn = connect(server, None).await?;
    let req = RegisterRequest { server_key: server.server_key.clone(), csr_pem: csr, hostname: platform.hostname() };
    conn.send(&AgentMessage { msg: Some(agent_message::Msg::Register(req)) }).await?;
    match conn.recv::<ServerMessage>().await?.and_then(|m| m.msg) {
        Some(server_message::Msg::Registered(r)) => {
            // The verifier accepted the chain only because it held the pinned CA; the Server sends it here.
            let ca_der = CertificateDer::from_pem_slice(r.ca_pem.as_bytes())?.to_vec();
            anyhow::ensure!(
                hex::encode(sha256(&ca_der)) == server_key.fingerprint,
                "the Server CA's fingerprint does not match the Server Key; refusing to register"
            );
            Ok(Credentials {
                device_id: r.device_id,
                cert_pem: r.cert_pem,
                key_pem: key.serialize_pem(),
                ca_der,
                update_key: r.update_key,
                device_password: r.device_password,
            })
        }
        Some(server_message::Msg::Error(e)) => anyhow::bail!("registration rejected: {}: {}", e.code, e.message),
        Some(_) | None => anyhow::bail!("server closed the connection during registration"),
    }
}

/// Removes this Device from the Server, what `sudo rcli agent uninstall` does before it deletes the local
/// credentials. It authenticates with the Device's certificate, not the Server Key: the Device may already have
/// forgotten the key, and the Server only needs to know which Device asks.
pub async fn unregister(server: &ServerAccess, creds: &Credentials) -> anyhow::Result<()> {
    unregister_at(&server.addr, creds).await
}

/// `unregister` at a bare `host:port`, trusted through the CA the Device pinned at registration.
pub async fn unregister_at(addr: &str, creds: &Credentials) -> anyhow::Result<()> {
    let fingerprint = hex::encode(sha256(&creds.ca_der));
    let config = config_pinned(&fingerprint, Some(creds))?;
    let mut conn = connect_with_config(addr, config).await?;
    conn.send(&AgentMessage { msg: Some(agent_message::Msg::Unregister(UnregisterRequest {})) }).await?;
    // The Server deletes the Device and closes the connection, whether or not the row still existed.
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.recv::<ServerMessage>()).await;
    Ok(())
}

/// Keeps a connection to the Server open forever, reconnecting with backoff.
/// Resources are sampled every 5 s whether connected or not; samples taken offline are backfilled on reconnect.
/// With a `status`, a changed Server Address or Server Key reconnects at once and Device identity changes go over the connection.
pub async fn run(server: ServerAccess, mut creds: Credentials, platform: Arc<dyn Platform>, status: Option<Arc<ipc::Status>>) {
    // With IPC, the Status owns the Server Address and Server Key (a change reconnects) and receives the changes to forward.
    let mut server_access = status.as_ref().map_or(server, |s| s.server_access());
    let mut inbox = Inbox {
        server_access: status.as_ref().map(|s| s.subscribe()),
        changes: status.as_ref().and_then(|s| s.take_changes()),
        disconnects: status.as_ref().and_then(|s| s.take_disconnects()),
    };
    let backlog = Arc::new(Backlog::default());
    let sampler = tokio::spawn(sample_forever(platform.clone(), backlog.clone()));
    // Stop the sampler when `run` is dropped (e.g. aborted).
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _sampler = AbortOnDrop(sampler);
    let agent = AgentState {
        // Outlives sessions: the last acted-on display and what each Controller last saw survive a reconnect.
        executor: Arc::new(Mutex::new(Executor::default())),
        // Live streams are peer-to-peer, so they survive a lost Server connection.
        live: live::Live::new(platform.clone()),
        media: Arc::new(media::Media::default()),
        streams: Default::default(),
        backlog,
        // Device Settings survive reconnects; with no Status (no IPC) the defaults apply.
        settings: status.as_ref().map_or_else(|| Arc::new(settings::Handle::new()), |s| s.settings()),
        // A root grant lasts one session (one connection to the Server) and only for the Controller it was given to.
        grant: status.as_ref().map_or_else(|| Arc::new(Grant::default()), |s| s.grant()),
        // Connected Controllers outlive a reconnect: they end on `disconnect`, not when the Server link blips.
        controllers: status.as_ref().map_or_else(|| Arc::new(Controllers::default()), |s| s.controllers()),
        platform,
        status,
    };
    let mut backoff = Duration::from_secs(1);
    loop {
        // Another Server Address is one switch away; pick up whichever is current.
        if let Some(rx) = &mut inbox.server_access {
            server_access = rx.borrow_and_update().clone();
        }
        let mut reconnect = false;
        match connect(&server_access, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                // A session grant belongs to the session that ended with the last connection.
                agent.grant.clear();
                if let Some(s) = &agent.status {
                    s.set_connected(true);
                }
                match session(conn, &mut creds, &agent, &mut inbox).await {
                    Ok(now) => reconnect = now,
                    Err(e) => tracing::warn!("connection to server lost: {e}"),
                }
                if let Some(s) = &agent.status {
                    s.set_connected(false);
                }
            }
            Err(e) => {
                if let Some(s) = &agent.status {
                    s.set_connected(false);
                }
                tracing::warn!("cannot connect to server: {e}");
            }
        }
        // A new Server Address reconnects at once; otherwise wait out the backoff.
        if reconnect {
            continue;
        }
        match &mut inbox.server_access {
            Some(rx) => tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                changed = rx.changed() => if changed.is_ok() { continue },
            },
            None => tokio::time::sleep(backoff).await,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// What outlives one connection to the Server: every session of `run` shares it.
struct AgentState {
    platform: Arc<dyn Platform>,
    executor: Arc<Mutex<Executor>>,
    live: Arc<live::Live>,
    media: Arc<media::Media>,
    /// Who opened each live stream and media channel (by session), so a `disconnect` ends only theirs.
    // shortcut: a stream that ends on its own stays listed until `disconnect` or LiveClose; closing it is a no-op.
    streams: Arc<Mutex<std::collections::HashMap<String, String>>>,
    backlog: Arc<Backlog>,
    status: Option<Arc<ipc::Status>>,
    settings: Arc<settings::Handle>,
    grant: Arc<Grant>,
    controllers: Arc<Controllers>,
}

/// What the person at the Device sends the running Agent over the IPC; all `None` without a Status.
struct Inbox {
    /// A new Server Address or Key.
    server_access: Option<tokio::sync::watch::Receiver<ServerAccess>>,
    /// Device identity changes to forward to the Server.
    changes: Option<tokio::sync::mpsc::UnboundedReceiver<ipc::Change>>,
    /// Controllers the person at the Device disconnected.
    disconnects: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
}

/// Waits until the Server Address changes; never, without a Status.
async fn server_access_changed(rx: &mut Option<tokio::sync::watch::Receiver<ServerAccess>>) -> bool {
    match rx {
        Some(rx) => rx.changed().await.is_ok(),
        None => std::future::pending().await,
    }
}

/// The next message on an IPC channel; never, without a Status.
async fn next<T>(rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn sample_forever(platform: Arc<dyn Platform>, backlog: Arc<Backlog>) {
    let mut sample = tokio::time::interval(SAMPLE_EVERY);
    loop {
        sample.tick().await;
        match sample_resources(platform.as_ref()) {
            Ok(r) => backlog.push(std::time::Instant::now(), r),
            Err(e) => tracing::warn!("cannot sample resources: {e:#}"),
        }
    }
}

async fn session(
    mut conn: Conn<TlsStream<TcpStream>>,
    creds: &mut Credentials,
    agent: &AgentState,
    inbox: &mut Inbox,
) -> anyhow::Result<bool> {
    let platform = agent.platform.as_ref();
    let status = agent.status.as_deref();
    let hello = AgentHello { version: platform.agent_version(), target: target() };
    let send = |msg| AgentMessage { msg: Some(msg) };
    conn.send(&send(agent_message::Msg::Hello(hello))).await?;
    let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
    let mut sample = tokio::time::interval(SAMPLE_EVERY);
    // What the Server has seen this session; a new session starts from nothing, so it gets everything.
    let (mut inventory, mut resources) = (None, Resources::default());
    // Replies from work that runs off this loop (Actions block on the OS for up to seconds; prompts for 30 s).
    let (replies, mut replies_rx) = tokio::sync::mpsc::unbounded_channel();
    // Device identity changes sent to the Server, awaiting its answer by request_id.
    let mut pending: std::collections::HashMap<u64, tokio::sync::oneshot::Sender<Result<String, String>>> = Default::default();
    // Shells are opened off this loop (a prompt may wait 30 s), so the map is shared with those tasks.
    let shells = Arc::new(Mutex::new(Shells::default()));
    // What every request that may prompt at the Device needs, cloned into the task that serves it.
    let gate = || Gate {
        platform: agent.platform.clone(),
        settings: agent.settings.get(),
        controllers: agent.controllers.clone(),
        replies: replies.clone(),
    };
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
            }
            _ = agent.backlog.ready() => {
                // shortcut: samples taken from the backlog are lost if this connection dies mid-send (<= a few);
                // re-queue them on error if that ever shows up as gaps.
                for (at, cur) in agent.backlog.take() {
                    let age_ms = at.elapsed().as_millis() as u64;
                    let sample = ResourceSample { resources: Some(delta(&resources, &cur)), age_ms };
                    conn.send(&send(agent_message::Msg::Sample(sample))).await?;
                    resources = cur;
                }
            }
            Some(reply) = replies_rx.recv() => {
                if let agent_message::Msg::ShellExit(e) = &reply {
                    shells.lock().unwrap().0.remove(&e.session);
                }
                conn.send(&send(reply)).await?
            }
            Some(change) = next(&mut inbox.changes) => {
                pending.insert(change.update.request_id, change.reply);
                conn.send(&send(agent_message::Msg::IdentityUpdate(change.update))).await?;
            }
            Some(controller) = next(&mut inbox.disconnects) => {
                // End this Controller's shells, live streams and media channels; the Server releases its Control Lock.
                shells.lock().unwrap().end(&controller);
                let ended: Vec<String> = {
                    let mut streams = agent.streams.lock().unwrap();
                    let ended = streams.iter().filter(|(_, c)| **c == controller).map(|(s, _)| s.clone()).collect();
                    streams.retain(|_, c| *c != controller);
                    ended
                };
                for session in ended {
                    agent.live.close(&session).await;
                    agent.media.close(&session).await;
                }
                conn.send(&send(agent_message::Msg::DisconnectController(DisconnectController { controller }))).await?;
            }
            // A new Server Address or Key: drop this connection so `run` connects with the new values.
            _ = server_access_changed(&mut inbox.server_access) => return Ok(true),
            msg = conn.recv::<ServerMessage>() => {
                match msg?.and_then(|m| m.msg) {
                    None => anyhow::bail!("server closed the connection"),
                    Some(server_message::Msg::Error(e)) => anyhow::bail!("server refused: {}: {}", e.code, e.message),
                    Some(server_message::Msg::Act(mut req)) => {
                        // The Executor's lock runs requests one at a time, in arrival order.
                        let (platform, executor, replies) = (agent.platform.clone(), agent.executor.clone(), replies.clone());
                        let (media, rt) = (agent.media.clone(), tokio::runtime::Handle::current());
                        let secrets = status.map(|s| s.secrets());
                        let gate = gate();
                        tokio::task::spawn_blocking(move || {
                            let failed = |error| {
                                let _ = replies.send(agent_message::Msg::ActResult(rcli_protocol::agent::ActResult {
                                    request_id: req.request_id,
                                    error: Some(error),
                                    ..Default::default()
                                }));
                            };
                            if let Err(e) = gate.admit(&req.controller) {
                                return failed(e);
                            }
                            // `unlock`: type this Device's own password; the Server never has it.
                            if req.unlock {
                                match unlock_actions(secrets.as_deref()) {
                                    Ok(actions) => req.actions = actions,
                                    Err(e) => return failed(e),
                                }
                            }
                            let mut result = executor.lock().unwrap().act(platform.as_ref(), &req);
                            if !req.media_session.is_empty() {
                                rt.block_on(media.offload(&req.media_session, &mut result));
                            }
                            let _ = replies.send(agent_message::Msg::ActResult(result));
                        });
                    }
                    Some(server_message::Msg::LiveOffer(offer)) => {
                        // Answering waits for ICE gathering (and maybe an accept prompt); keep the loop running.
                        let (live, media, streams, replies) = (agent.live.clone(), agent.media.clone(), agent.streams.clone(), replies.clone());
                        let gate = gate();
                        tokio::spawn(async move {
                            let controller = offer.controller.clone();
                            let admitted = tokio::task::spawn_blocking(move || gate.admit(&controller)).await;
                            let answer = match admitted {
                                Ok(Ok(())) => {
                                    streams.lock().unwrap().insert(offer.session.clone(), offer.controller.clone());
                                    if offer.media { media.open(offer).await } else { live.open(offer).await }
                                }
                                Ok(Err(error)) => LiveAnswer { request_id: offer.request_id, error: Some(error), ..Default::default() },
                                Err(e) => LiveAnswer { request_id: offer.request_id, error: Some(platform_error(e)), ..Default::default() },
                            };
                            let _ = replies.send(agent_message::Msg::LiveAnswer(answer));
                        });
                    }
                    Some(server_message::Msg::LiveClose(c)) => {
                        agent.streams.lock().unwrap().remove(&c.session);
                        let (live, media) = (agent.live.clone(), agent.media.clone());
                        tokio::spawn(async move {
                            live.close(&c.session).await;
                            media.close(&c.session).await;
                        });
                    }
                    Some(server_message::Msg::Exec(req)) => {
                        // Off the loop: a command may run for minutes; commands run concurrently.
                        let grant = agent.grant.clone();
                        let gate = gate();
                        tokio::task::spawn_blocking(move || {
                            let _ = gate.replies.send(agent_message::Msg::ExecResult(exec(&gate, &req, &grant)));
                        });
                    }
                    Some(server_message::Msg::ShellOpen(req)) => {
                        // Off the loop: the accept and root prompts may each wait 30 s.
                        let (grant, shells) = (agent.grant.clone(), shells.clone());
                        let gate = gate();
                        tokio::task::spawn_blocking(move || open_shell(&gate, &req, &grant, &shells));
                    }
                    Some(server_message::Msg::ShellInput(i)) => {
                        // shortcut: written inline; a PTY whose buffer is full (shell not reading) would stall this loop.
                        if let Some(shell) = shells.lock().unwrap().0.get_mut(&i.session) {
                            let _ = std::io::Write::write_all(&mut shell.input, &i.data);
                        }
                    }
                    Some(server_message::Msg::ShellResize(r)) => {
                        if let Some(shell) = shells.lock().unwrap().0.get(&r.session) {
                            let _ = shell.control.lock().unwrap().resize(clamp_u16(r.cols), clamp_u16(r.rows));
                        }
                    }
                    Some(server_message::Msg::ShellClose(c)) => {
                        if let Some(shell) = shells.lock().unwrap().0.remove(&c.session) {
                            let _ = shell.control.lock().unwrap().kill();
                        }
                    }
                    Some(server_message::Msg::ListDisplays(req)) => {
                        let (displays, error) = match platform.displays() {
                            Ok(d) => (d, None),
                            Err(e) => (vec![], Some(Error { code: "platform".into(), message: e.to_string() })),
                        };
                        let reply = DisplayList { request_id: req.request_id, displays, error };
                        conn.send(&send(agent_message::Msg::Displays(reply))).await?;
                    }
                    Some(server_message::Msg::ListProcesses(req)) => {
                        let reply = match platform.processes() {
                            Ok(processes) => ProcessList { request_id: req.request_id, processes, error: String::new() },
                            Err(e) => ProcessList { request_id: req.request_id, processes: vec![], error: format!("{e:#}") },
                        };
                        conn.send(&send(agent_message::Msg::Processes(reply))).await?;
                    }
                    Some(server_message::Msg::Wake(req)) => {
                        let sent = <[u8; 6]>::try_from(req.mac.as_slice())
                            .map_err(|_| anyhow::anyhow!("MAC must be 6 bytes"))
                            .and_then(|mac| platform.send_wake(mac));
                        let error = sent.err().map(|e| Error { code: "platform".into(), message: format!("{e:#}") });
                        let reply = WakeResult { request_id: req.request_id, error };
                        conn.send(&send(agent_message::Msg::WakeResult(reply))).await?;
                    }
                    Some(server_message::Msg::Power(req)) => {
                        use rcli_protocol::agent::PowerAction as P;
                        let action = match req.action() {
                            P::Restart => platform::PowerAction::Restart,
                            P::Shutdown => platform::PowerAction::Shutdown,
                            P::Sleep => platform::PowerAction::Sleep,
                            P::LockScreen => platform::PowerAction::Lock,
                        };
                        // Off the loop: adapters may take a moment to hand the request to the OS.
                        let gate = gate();
                        tokio::task::spawn_blocking(move || {
                            let done = gate.admit(&req.controller).and_then(|()| {
                                gate.platform.power(action).map_err(|e| Error { code: "platform".into(), message: format!("{e:#}") })
                            });
                            let reply = PowerResult { request_id: req.request_id, error: done.err() };
                            let _ = gate.replies.send(agent_message::Msg::PowerResult(reply));
                        });
                    }
                    Some(server_message::Msg::Update(update)) => {
                        // shortcut: installs inline, blocking heartbeats; fine while installs take well under 30 s.
                        match install(creds, platform, &update) {
                            // Reconnect so the Server sees the new version (real adapters restart instead).
                            Ok(()) => return Ok(true),
                            Err(e) => {
                                tracing::warn!("rejected Agent update {}: {e:#}", update.version);
                                let failed = UpdateFailed { version: update.version, reason: format!("{e:#}") };
                                conn.send(&send(agent_message::Msg::UpdateFailed(failed))).await?;
                            }
                        }
                    }
                    Some(server_message::Msg::ControlIndicator(c)) => {
                        agent.live.set_lock_indicator(c.on);
                    }
                    Some(server_message::Msg::IdentityChange(u)) => {
                        let reply = server_identity_change(&u, status, creds);
                        conn.send(&send(agent_message::Msg::IdentityUpdated(reply))).await?;
                    }
                    Some(server_message::Msg::IdentityUpdated(r)) => {
                        if let Some(reply) = pending.remove(&r.request_id) {
                            let result = match r.error {
                                Some(e) => Err(e.message),
                                None => {
                                    if let Some(s) = status {
                                        s.set_device_id(&r.device_id);
                                    }
                                    creds.device_id = r.device_id.clone();
                                    Ok(r.device_id)
                                }
                            };
                            let _ = reply.send(result);
                        }
                    }
                    // Later tickets handle more requests from the Server here.
                    Some(_) => {}
                }
            }
        }
    }
}

fn platform_error(e: impl std::fmt::Display) -> Error {
    Error { code: "platform".into(), message: e.to_string() }
}

/// How long the person at the Device has to answer a root prompt.
const ROOT_PROMPT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the person at the Device has to accept a new connection.
const ACCEPT_PROMPT_TIMEOUT: Duration = Duration::from_secs(30);

/// The Actions `unlock` types: the Device's own unlock password, then Enter. None set is a typed error.
fn unlock_actions(secrets: Option<&dyn secret_store::SecretStore>) -> Result<Vec<rcli_protocol::agent::Action>, Error> {
    let Some(secrets) = secrets else {
        return Err(not_set("this Agent has no Secret Store, so it has no unlock password"));
    };
    match secrets.get(ipc::UNLOCK_PASSWORD) {
        Ok(Some(password)) => Ok(vec![
            rcli_protocol::agent::Action { kind: Some(rcli_protocol::agent::action::Kind::TypeText(password)) },
            rcli_protocol::agent::Action { kind: Some(rcli_protocol::agent::action::Kind::Key("enter".into())) },
        ]),
        Ok(None) => Err(not_set("no unlock password is set on this Device; set one with `sudo rcli agent unlock-password set`")),
        Err(e) => Err(Error { code: "secret_store".into(), message: format!("cannot read the unlock password: {e:#}") }),
    }
}

fn not_set(message: &str) -> Error {
    Error { code: "unlock_password_not_set".into(), message: message.into() }
}

/// Whether the Controller display name `name` (e.g. "alice (Dashboard)", "dev-laptop (10.0.0.5)") is the one the
/// person at the Device means by `who`: the full name, or the part before " (".
fn names(name: &str, who: &str) -> bool {
    name == who || name.strip_prefix(who).is_some_and(|rest| rest.starts_with(" ("))
}

/// Whether a root grant given as `who` covers the Controller display name `controller`. Unlike `names`, a bare name
/// covers only the Dashboard User of that name (the Server authenticated it): a password connection picks its own
/// machine name, so only its full display name `<host> (<IP>)` covers it.
fn granted(controller: &str, who: &str) -> bool {
    controller == who || controller == format!("{who} (Dashboard)")
}

/// Root granted to the current session: one Controller, for as long as the Agent's connection to the Server lasts
/// or until the person at the Device disconnects that Controller. The person at the Device (or its GUI) gives it.
#[derive(Default)]
pub struct Grant(Mutex<Option<String>>);

impl Grant {
    /// Grants root to the Controller the person calls `who` (see `granted`) for this session only.
    pub fn grant(&self, who: &str) {
        *self.0.lock().unwrap() = Some(who.to_owned());
    }

    fn holds(&self, controller: &str) -> bool {
        self.0.lock().unwrap().as_deref().is_some_and(|who| granted(controller, who))
    }

    /// Ends the grant if `controller` holds it: a disconnected Controller does not keep root when it reconnects.
    pub fn revoke(&self, controller: &str) {
        let mut grant = self.0.lock().unwrap();
        if grant.as_deref().is_some_and(|who| granted(controller, who)) {
            *grant = None;
        }
    }

    /// Ends the grant, at the end of the session it was given for.
    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

/// Controllers using this Device right now, keyed by their display name (as `rcli agent info` lists them), with
/// whether each has been accepted at the Device (only matters in `password+accept` mode). Survives a reconnect.
#[derive(Default)]
pub struct Controllers(std::sync::Mutex<std::collections::HashMap<String, bool>>);

impl Controllers {
    /// Records `name` as connected (keeping any earlier accept).
    fn touch(&self, name: &str) {
        self.0.lock().unwrap().entry(name.to_owned()).or_insert(false);
    }

    fn accept(&self, name: &str) {
        self.0.lock().unwrap().insert(name.to_owned(), true);
    }

    fn accepted(&self, name: &str) -> bool {
        self.0.lock().unwrap().get(name).copied().unwrap_or(false)
    }

    /// The display names of the connected Controllers.
    pub fn list(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }

    /// Removes the Controllers the person calls `who` (see `names`), returning the display names that were ended.
    pub fn remove(&self, who: &str) -> Vec<String> {
        let mut map = self.0.lock().unwrap();
        let removed: Vec<String> = map.keys().filter(|n| names(n, who)).cloned().collect();
        for name in &removed {
            map.remove(name);
        }
        removed
    }

    /// Removes every Controller, returning their display names.
    pub fn clear(&self) -> Vec<String> {
        self.0.lock().unwrap().drain().map(|(k, _)| k).collect()
    }
}

/// How a request that can prompt the person at the Device was decided, as the audit records it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Decision {
    /// Root: the setting is `always`.
    Always,
    /// Root: the person granted it to this session.
    Session,
    Accepted,
    Rejected,
    /// Nobody is logged in to show the prompt to.
    NoSession,
    /// Root: the setting is `never`.
    Never,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Always => "always",
            Decision::Session => "session",
            Decision::Accepted => "accepted",
            Decision::Rejected => "rejected",
            Decision::NoSession => "no_session",
            Decision::Never => "never",
        }
    }
}

/// The wire form of a decision: empty when nothing was decided (no prompt, root not asked for).
fn decision_name(decision: Option<Decision>) -> String {
    decision.map_or("", Decision::as_str).into()
}

/// Prompts appear in the logged-in session; with nobody logged in there is no session to show one in.
fn nobody_to_ask(platform: &dyn Platform) -> bool {
    platform.session_state().is_ok_and(|s| s != platform::SessionState::LoggedIn)
}

/// A refused request and how it was decided (for the audit entry).
type Refusal = (Error, Option<Decision>);

/// Who a terminal command or session runs as: the Device's Terminal User, or root when asked and allowed.
/// `grant` is the current session's root grant, held by `controller` (a display name). `requester` is who the root
/// prompt names. Returns the account and how root was decided (for the audit entry).
fn account_for(
    platform: &dyn Platform,
    settings: &settings::DeviceSettings,
    as_root: bool,
    requester: &str,
    controller: &str,
    grant: &Grant,
) -> Result<(platform::Account, Option<Decision>), Refusal> {
    let denied = |message: &str, decision| (Error { code: "root_denied".into(), message: message.into() }, Some(decision));
    if !as_root {
        return Ok((platform::Account::User(settings.terminal_user.clone()), None));
    }
    match settings.root {
        settings::RootAccess::Always => return Ok((platform::Account::Root, Some(Decision::Always))),
        settings::RootAccess::Never => return Err(denied("root access is turned off on this Device", Decision::Never)),
        settings::RootAccess::Ask => {}
    }
    if grant.holds(controller) {
        return Ok((platform::Account::Root, Some(Decision::Session)));
    }
    if nobody_to_ask(platform) {
        return Err(denied("root access needs a person at the Device to accept, and nobody can be asked right now", Decision::NoSession));
    }
    match platform.ask_root(requester, ROOT_PROMPT_TIMEOUT) {
        Ok(true) => Ok((platform::Account::Root, Some(Decision::Accepted))),
        Ok(false) => Err(denied("the person at the Device did not accept root access", Decision::Rejected)),
        Err(e) => Err((platform_error(format!("cannot ask at the Device: {e:#}")), None)),
    }
}

/// What a request that may prompt the person at the Device needs, off the session loop.
struct Gate {
    platform: Arc<dyn Platform>,
    settings: settings::DeviceSettings,
    controllers: Arc<Controllers>,
    replies: tokio::sync::mpsc::UnboundedSender<agent_message::Msg>,
}

impl Gate {
    /// Admits `controller` (a display name) to any request on this Device and lists it as connected. In
    /// `password+accept` mode a person at the Device accepts each new Controller once (re-asked after a
    /// `disconnect`); refused when no session can show the prompt. A prompt's answer goes to the Server's audit.
    fn admit(&self, controller: &str) -> Result<(), Error> {
        self.controllers.touch(controller);
        if self.settings.accept == settings::AcceptMode::Password || self.controllers.accepted(controller) {
            return Ok(());
        }
        let denied = |message: &str| Error { code: "connect_denied".into(), message: message.into() };
        let (result, decision) = if nobody_to_ask(self.platform.as_ref()) {
            (Err(denied("a person at the Device must accept this connection, and nobody can be asked right now")), Decision::NoSession)
        } else {
            match self.platform.ask_accept(controller, ACCEPT_PROMPT_TIMEOUT) {
                Ok(true) => {
                    self.controllers.accept(controller);
                    (Ok(()), Decision::Accepted)
                }
                Ok(false) => (Err(denied("the person at the Device did not accept this connection")), Decision::Rejected),
                Err(e) => return Err(platform_error(format!("cannot ask at the Device: {e:#}"))),
            }
        };
        let answer = ConnectDecision { controller: controller.into(), decision: decision.as_str().into() };
        let _ = self.replies.send(agent_message::Msg::ConnectDecision(answer));
        result
    }

    /// Admits `controller`, then decides the account its command or shell runs as (see `account_for`).
    fn account(&self, controller: &str, as_root: bool, requester: &str, grant: &Grant) -> Result<(platform::Account, Option<Decision>), Refusal> {
        self.admit(controller).map_err(|e| (e, None))?;
        account_for(self.platform.as_ref(), &self.settings, as_root, requester, controller, grant)
    }
}

/// Applies an Admin's identity change from the Dashboard: writes a new Device Password to the Secret Store and keeps
/// a new Device ID. The Server has already checked and stored its side; `error` tells it to leave the Server side
/// alone (nothing is half-applied here: the password is stored first and an ID change never writes the password).
fn server_identity_change(u: &IdentityUpdate, status: Option<&ipc::Status>, creds: &mut Credentials) -> IdentityUpdated {
    let (password, device_id) = (&u.device_password, &u.device_id);
    let error = match status {
        Some(s) if !password.is_empty() => s.store_password(password).err().map(|e| Error { code: "secret_store".into(), message: format!("{e:#}") }),
        None if !password.is_empty() => Some(Error { code: "no_secret_store".into(), message: "the Agent has no Secret Store".into() }),
        _ => None,
    };
    if error.is_some() {
        return IdentityUpdated { request_id: u.request_id, error, device_id: String::new() };
    }
    if !device_id.is_empty() {
        if let Some(s) = status {
            s.set_device_id(device_id);
        }
        creds.device_id = device_id.clone();
    }
    let in_effect = if device_id.is_empty() { creds.device_id.clone() } else { device_id.clone() };
    IdentityUpdated { request_id: u.request_id, error: None, device_id: in_effect }
}

/// Runs a one-shot command (see `account_for`).
fn exec(gate: &Gate, req: &ExecRequest, grant: &Grant) -> ExecResult {
    let failed = |error, decision| ExecResult {
        request_id: req.request_id,
        error: Some(error),
        root_decision: decision_name(decision),
        ..Default::default()
    };
    let (account, decision) = match gate.account(&req.controller, req.as_root, &req.requester, grant) {
        Ok(r) => r,
        Err((e, decision)) => return failed(e, decision),
    };
    let as_root = account == platform::Account::Root;
    match gate.platform.shell(&req.command, account, Duration::from_millis(req.timeout_ms.into())) {
        Ok(out) => ExecResult {
            request_id: req.request_id,
            exit_code: out.exit_code,
            stdout: out.stdout,
            stderr: out.stderr,
            timed_out: out.timed_out,
            as_root,
            error: None,
            root_decision: decision_name(decision),
        },
        Err(e) => failed(platform_error(format!("{e:#}")), decision),
    }
}

/// A live PTY session and the Controller (display name) that opened it.
struct Shell {
    input: Box<dyn std::io::Write + Send>,
    control: SharedControl,
    controller: String,
}

/// Live PTY sessions of this connection, by session id. Dropping it (connection lost) kills them all.
#[derive(Default)]
struct Shells(std::collections::HashMap<String, Shell>);

type SharedControl = Arc<Mutex<Box<dyn platform::PtyControl>>>;

impl Shells {
    /// Kills the sessions `controller` opened (the person at the Device disconnected it).
    fn end(&mut self, controller: &str) {
        self.0.retain(|_, shell| {
            if shell.controller != controller {
                return true;
            }
            let _ = shell.control.lock().unwrap().kill();
            false
        });
    }
}

impl Drop for Shells {
    fn drop(&mut self) {
        for shell in self.0.values() {
            let _ = shell.control.lock().unwrap().kill();
        }
    }
}

/// Starts a PTY session (see `account_for`) and answers with ShellOpened; a thread then relays its output, then its exit
/// code, through the gate's replies.
fn open_shell(gate: &Gate, req: &ShellOpen, grant: &Grant, shells: &Mutex<Shells>) {
    let opened = |as_root, error, decision| {
        let reply = ShellOpened { request_id: req.request_id, as_root, error, root_decision: decision_name(decision) };
        let _ = gate.replies.send(agent_message::Msg::ShellOpened(reply));
    };
    let (account, decision) = match gate.account(&req.controller, req.as_root, &req.requester, grant) {
        Ok(r) => r,
        Err((e, decision)) => return opened(false, Some(e), decision),
    };
    let as_root = account == platform::Account::Root;
    let pty = match gate.platform.pty(account, clamp_u16(req.cols), clamp_u16(req.rows)) {
        Ok(p) => p,
        Err(e) => return opened(false, Some(platform_error(format!("{e:#}"))), decision),
    };
    let control = Arc::new(Mutex::new(pty.control));
    let shell = Shell { input: pty.input, control: control.clone(), controller: req.controller.clone() };
    shells.lock().unwrap().0.insert(req.session.clone(), shell);
    // Answered before the first output, so the Server sees the session open first.
    opened(as_root, None, decision);
    let (session, replies, mut output) = (req.session.clone(), gate.replies.clone(), pty.output);
    std::thread::spawn(move || {
        let mut buf = vec![0; 16 * 1024];
        while let Ok(n @ 1..) = output.read(&mut buf) {
            let msg = agent_message::Msg::ShellOutput(ShellOutput { session: session.clone(), data: buf[..n].to_vec() });
            if replies.send(msg).is_err() {
                return; // connection gone; `Shells` was dropped and killed the PTY
            }
        }
        let exit_code = control.lock().unwrap().wait().unwrap_or(-1);
        let _ = replies.send(agent_message::Msg::ShellExit(ShellExit { session, exit_code }));
    });
}

fn clamp_u16(n: u32) -> u16 {
    n.clamp(1, u16::MAX.into()) as u16
}

/// Verifies `update` against the update key pinned at registration and the target, then installs it.
fn install(creds: &Credentials, platform: &dyn Platform, update: &AgentUpdate) -> anyhow::Result<()> {
    anyhow::ensure!(!creds.update_key.is_empty(), "no update key pinned on this Agent");
    let key = &creds.update_key;
    anyhow::ensure!(update.target == target(), "build is for {}, this Agent is {}", update.target, target());
    anyhow::ensure!(
        rcli_protocol::update::verify(key, &update.version, &update.target, &update.build, &update.signature),
        "bad signature: build is not signed with this Agent's update key"
    );
    platform.install_update(&update.version, &update.build)
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
