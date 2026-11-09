//! Agent core: registration and the persistent connection to the Server.

pub mod backlog;
pub mod contract;
mod clip;
mod coords;
mod executor;
pub mod ipc;
mod live;
mod marks;
mod media;
mod observer;
pub mod platform;
pub mod secret_store;
pub mod settings;

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
        agent_message, server_message, AgentHello, AgentMessage, AgentUpdate, DisplayList, Error, ExecRequest,
        ExecResult, Heartbeat, PowerResult, ProcessList, RegisterRequest, ResourceSample, Resources, ServerMessage,
        ShellExit, ShellOpen, ShellOpened, ShellOutput, UpdateFailed, WakeResult,
    },
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
pub struct ServerEndpoint {
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

/// `rk_<fingerprint>_<secret>` as `(fingerprint, secret)`.
fn split_server_key(key: &str) -> anyhow::Result<(&str, &str)> {
    let (fingerprint, secret) = key
        .trim()
        .strip_prefix("rk_")
        .and_then(|rest| rest.split_once('_'))
        .ok_or_else(|| anyhow::anyhow!("the Server Key must look like rk_<fingerprint>_<secret>"))?;
    Ok((fingerprint, secret))
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

fn config(server: &ServerEndpoint, creds: Option<&Credentials>) -> anyhow::Result<ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no crypto provider"))?;
    let builder = ClientConfig::builder().dangerous().with_custom_certificate_verifier(std::sync::Arc::new(PinnedCa {
        fingerprint: split_server_key(&server.server_key)?.0.to_owned(),
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

async fn connect(server: &ServerEndpoint, creds: Option<&Credentials>) -> anyhow::Result<Conn<TlsStream<TcpStream>>> {
    let config = config(server, creds)?;
    let host = server.addr.rsplit_once(':').map_or(server.addr.as_str(), |(h, _)| h);
    let name = ServerName::try_from(host.trim_matches(['[', ']']).to_owned())?;
    let tcp = TcpStream::connect(&server.addr).await?;
    let tls = TlsConnector::from(Arc::new(config)).connect(name, tcp).await?;
    Ok(Conn::with_max_frame(tls, rcli_protocol::update::MAX_SESSION_FRAME))
}

/// Registers this Device with the Server Key: checks the Server's CA against the key's fingerprint, presents its
/// secret, and gets back this Device's mTLS certificate, 9-digit Device ID and update key to pin.
/// The private key never leaves the Agent; only a CSR is sent.
pub async fn register(server: &ServerEndpoint, platform: &dyn Platform) -> anyhow::Result<Credentials> {
    split_server_key(&server.server_key)?;
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
                hex::encode(sha256(&ca_der)) == split_server_key(&server.server_key)?.0.to_lowercase(),
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

/// Keeps a connection to the Server open forever, reconnecting with backoff.
/// Resources are sampled every 5 s whether connected or not; samples taken offline are backfilled on reconnect.
/// With a `status`, a changed Server endpoint reconnects at once and Device identity changes go over the connection.
pub async fn run(server: ServerEndpoint, mut creds: Credentials, platform: Arc<dyn Platform>, status: Option<Arc<ipc::Status>>) {
    // With IPC, the Status owns the endpoint (a change reconnects) and receives the changes to forward.
    let mut endpoint = status.as_ref().map_or(server, |s| s.endpoint());
    let mut endpoints = status.as_ref().map(|s| s.subscribe());
    let mut changes = status.as_ref().and_then(|s| s.take_changes());
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
    // Outlives sessions: the last acted-on display and what each Controller last saw survive a reconnect.
    let executor = Arc::new(Mutex::new(Executor::default()));
    // Live streams are peer-to-peer, so they survive a lost Server connection.
    let live = live::Live::new(platform.clone());
    let media = Arc::new(media::Media::default());
    let mut backoff = Duration::from_secs(1);
    loop {
        // Another Server Address is one switch away; pick up whichever is current.
        if let Some(rx) = &mut endpoints {
            endpoint = rx.borrow_and_update().clone();
        }
        let mut reconnect = false;
        match connect(&endpoint, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                if let Some(s) = &status {
                    s.set_connected(true);
                }
                match session(
                    conn,
                    &mut creds,
                    &platform,
                    &executor,
                    &live,
                    &media,
                    &backlog,
                    status.as_deref(),
                    &mut endpoints,
                    &mut changes,
                )
                .await
                {
                    Ok(now) => reconnect = now,
                    Err(e) => tracing::warn!("connection to server lost: {e}"),
                }
                if let Some(s) = &status {
                    s.set_connected(false);
                }
            }
            Err(e) => {
                if let Some(s) = &status {
                    s.set_connected(false);
                }
                tracing::warn!("cannot connect to server: {e}");
            }
        }
        // A new Server Address reconnects at once; otherwise wait out the backoff.
        if reconnect {
            continue;
        }
        match &mut endpoints {
            Some(rx) => tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                changed = rx.changed() => if changed.is_ok() { continue },
            },
            None => tokio::time::sleep(backoff).await,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Waits until the Server Address changes; never, without a Status.
async fn endpoints_changed(rx: &mut Option<tokio::sync::watch::Receiver<ServerEndpoint>>) -> bool {
    match rx {
        Some(rx) => rx.changed().await.is_ok(),
        None => std::future::pending().await,
    }
}

/// The next Device identity change to forward; never, without a Status.
async fn next_change(rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<ipc::Change>>) -> Option<ipc::Change> {
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

#[allow(clippy::too_many_arguments)]
async fn session(
    mut conn: Conn<TlsStream<TcpStream>>,
    creds: &mut Credentials,
    platform_arc: &Arc<dyn Platform>,
    executor: &Arc<Mutex<Executor>>,
    live: &Arc<live::Live>,
    media: &Arc<media::Media>,
    backlog: &Backlog,
    status: Option<&ipc::Status>,
    endpoints: &mut Option<tokio::sync::watch::Receiver<ServerEndpoint>>,
    changes: &mut Option<tokio::sync::mpsc::UnboundedReceiver<ipc::Change>>,
) -> anyhow::Result<bool> {
    let platform = platform_arc.as_ref();
    let hello = AgentHello { version: platform.agent_version(), target: target() };
    let send = |msg| AgentMessage { msg: Some(msg) };
    conn.send(&send(agent_message::Msg::Hello(hello))).await?;
    let mut tick = tokio::time::interval(HEARTBEAT_EVERY);
    let mut sample = tokio::time::interval(SAMPLE_EVERY);
    // What the Server has seen this session; a new session starts from nothing, so it gets everything.
    let (mut inventory, mut resources) = (None, Resources::default());
    // Replies from work that runs off this loop (Actions block on the OS for up to seconds).
    let (replies, mut replies_rx) = tokio::sync::mpsc::unbounded_channel();
    // Device identity changes sent to the Server, awaiting its answer by request_id.
    let mut pending: std::collections::HashMap<u64, tokio::sync::oneshot::Sender<Result<String, String>>> = Default::default();
    let mut shells = Shells::default();
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
            _ = backlog.ready() => {
                // shortcut: samples taken from the backlog are lost if this connection dies mid-send (<= a few);
                // re-queue them on error if that ever shows up as gaps.
                for (at, cur) in backlog.take() {
                    let age_ms = at.elapsed().as_millis() as u64;
                    let sample = ResourceSample { resources: Some(delta(&resources, &cur)), age_ms };
                    conn.send(&send(agent_message::Msg::Sample(sample))).await?;
                    resources = cur;
                }
            }
            Some(reply) = replies_rx.recv() => {
                if let agent_message::Msg::ShellExit(e) = &reply {
                    shells.0.remove(&e.session);
                }
                conn.send(&send(reply)).await?
            }
            Some(change) = next_change(changes) => {
                pending.insert(change.update.request_id, change.reply);
                conn.send(&send(agent_message::Msg::IdentityUpdate(change.update))).await?;
            }
            // A new Server Address or Key: drop this connection so `run` connects with the new values.
            _ = endpoints_changed(endpoints) => return Ok(true),
            msg = conn.recv::<ServerMessage>() => {
                match msg?.and_then(|m| m.msg) {
                    None => anyhow::bail!("server closed the connection"),
                    Some(server_message::Msg::Error(e)) => anyhow::bail!("server refused: {}: {}", e.code, e.message),
                    Some(server_message::Msg::Act(req)) => {
                        // The Executor's lock runs requests one at a time, in arrival order.
                        let (platform, executor, replies) = (platform_arc.clone(), executor.clone(), replies.clone());
                        let (media, rt) = (media.clone(), tokio::runtime::Handle::current());
                        tokio::task::spawn_blocking(move || {
                            let mut result = executor.lock().unwrap().act(platform.as_ref(), &req);
                            if !req.media_session.is_empty() {
                                rt.block_on(media.offload(&req.media_session, &mut result));
                            }
                            let _ = replies.send(agent_message::Msg::ActResult(result));
                        });
                    }
                    Some(server_message::Msg::LiveOffer(offer)) => {
                        // Answering waits for ICE gathering; keep the loop (heartbeats) running meanwhile.
                        let (live, media, replies) = (live.clone(), media.clone(), replies.clone());
                        tokio::spawn(async move {
                            let answer = if offer.media { media.open(offer).await } else { live.open(offer).await };
                            let _ = replies.send(agent_message::Msg::LiveAnswer(answer));
                        });
                    }
                    Some(server_message::Msg::LiveClose(c)) => {
                        let (live, media) = (live.clone(), media.clone());
                        tokio::spawn(async move {
                            live.close(&c.session).await;
                            media.close(&c.session).await;
                        });
                    }
                    Some(server_message::Msg::Exec(req)) => {
                        // Off the loop: a command may run for minutes; commands run concurrently.
                        let (platform, replies) = (platform_arc.clone(), replies.clone());
                        tokio::task::spawn_blocking(move || {
                            let _ = replies.send(agent_message::Msg::ExecResult(exec(platform.as_ref(), &req)));
                        });
                    }
                    Some(server_message::Msg::ShellOpen(req)) => {
                        let reply = open_shell(platform, &req, &replies, &mut shells);
                        conn.send(&send(agent_message::Msg::ShellOpened(reply))).await?;
                    }
                    Some(server_message::Msg::ShellInput(i)) => {
                        // shortcut: written inline; a PTY whose buffer is full (shell not reading) would stall this loop.
                        if let Some((input, _)) = shells.0.get_mut(&i.session) {
                            let _ = std::io::Write::write_all(input, &i.data);
                        }
                    }
                    Some(server_message::Msg::ShellResize(r)) => {
                        if let Some((_, control)) = shells.0.get(&r.session) {
                            let _ = control.lock().unwrap().resize(clamp_u16(r.cols), clamp_u16(r.rows));
                        }
                    }
                    Some(server_message::Msg::ShellClose(c)) => {
                        if let Some((_, control)) = shells.0.remove(&c.session) {
                            let _ = control.lock().unwrap().kill();
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
                        let (platform, replies) = (platform_arc.clone(), replies.clone());
                        tokio::task::spawn_blocking(move || {
                            let error = platform.power(action).err().map(|e| Error { code: "platform".into(), message: format!("{e:#}") });
                            let _ = replies.send(agent_message::Msg::PowerResult(PowerResult { request_id: req.request_id, error }));
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
                        live.set_lock_indicator(c.on);
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

/// Who a terminal command or session runs as: the logged-in user, or root when asked or allowed with nobody logged in.
fn run_as(platform: &dyn Platform, as_root: bool, root_if_nobody_logged_in: bool) -> Result<(platform::RunAs, bool), Error> {
    let nobody = match platform.session_state() {
        Ok(state) => state == platform::SessionState::LoginScreen,
        Err(e) => return Err(Error { code: "platform".into(), message: format!("{e:#}") }),
    };
    if nobody && !as_root && !root_if_nobody_logged_in {
        return Err(Error { code: "no_logged_in_user".into(), message: "nobody is logged in on the Device".into() });
    }
    let as_root = as_root || nobody;
    Ok((if as_root { platform::RunAs::Root } else { platform::RunAs::LoggedInUser }, as_root))
}

/// Runs a one-shot command (see `run_as`).
fn exec(platform: &dyn Platform, req: &ExecRequest) -> ExecResult {
    let reply = |error: Error| ExecResult { request_id: req.request_id, error: Some(error), ..Default::default() };
    let (run_as, as_root) = match run_as(platform, req.as_root, req.root_if_nobody_logged_in) {
        Ok(r) => r,
        Err(e) => return reply(e),
    };
    match platform.shell(&req.command, run_as, Duration::from_millis(req.timeout_ms.into())) {
        Ok(out) => ExecResult {
            request_id: req.request_id,
            exit_code: out.exit_code,
            stdout: out.stdout,
            stderr: out.stderr,
            timed_out: out.timed_out,
            as_root,
            error: None,
        },
        Err(e) => reply(Error { code: "platform".into(), message: format!("{e:#}") }),
    }
}

/// Live PTY sessions of this connection, by session id. Dropping it (connection lost) kills them all.
#[derive(Default)]
struct Shells(std::collections::HashMap<String, (Box<dyn std::io::Write + Send>, SharedControl)>);

type SharedControl = Arc<Mutex<Box<dyn platform::PtyControl>>>;

impl Drop for Shells {
    fn drop(&mut self) {
        for (_, control) in self.0.values() {
            let _ = control.lock().unwrap().kill();
        }
    }
}

/// Starts a PTY session (see `run_as`); a thread relays its output, then its exit code, through `replies`.
fn open_shell(
    platform: &dyn Platform,
    req: &ShellOpen,
    replies: &tokio::sync::mpsc::UnboundedSender<agent_message::Msg>,
    shells: &mut Shells,
) -> ShellOpened {
    let reply = |error: Error| ShellOpened { request_id: req.request_id, error: Some(error), ..Default::default() };
    let (run_as, as_root) = match run_as(platform, req.as_root, req.root_if_nobody_logged_in) {
        Ok(r) => r,
        Err(e) => return reply(e),
    };
    let pty = match platform.pty(run_as, clamp_u16(req.cols), clamp_u16(req.rows)) {
        Ok(p) => p,
        Err(e) => return reply(Error { code: "platform".into(), message: format!("{e:#}") }),
    };
    let control = Arc::new(Mutex::new(pty.control));
    shells.0.insert(req.session.clone(), (pty.input, control.clone()));
    let (session, replies, mut output) = (req.session.clone(), replies.clone(), pty.output);
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
    ShellOpened { request_id: req.request_id, as_root, error: None }
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
