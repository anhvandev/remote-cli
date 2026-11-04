//! Agent core: enrollment and the persistent connection to the Server.

pub mod backlog;
pub mod contract;
mod coords;
mod executor;
mod marks;
mod observer;
pub mod platform;

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
        agent_message, server_message, AgentHello, AgentMessage, AgentUpdate, DisplayList, EnrollRequest, Error,
        ExecRequest, ExecResult, Heartbeat, ProcessList, ResourceSample, Resources, ServerMessage, ShellExit, ShellOpen,
        ShellOpened, ShellOutput, UpdateFailed,
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
/// Inventory is checked and resources are sampled this often (each sample is a history point).
const SAMPLE_EVERY: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Where the Server's Agent port is and what to trust from it.
#[derive(Clone, Debug)]
pub struct ServerEndpoint {
    /// `host:port`; `host` must be one of the Server's `agent_hostnames`.
    pub addr: String,
    pub ca_pem: String,
    /// Raw Ed25519 public key Agent builds must be signed with; `None` rejects every update.
    pub update_key: Option<Vec<u8>>,
}

/// `<os>-<arch>` of this Agent binary; Agent builds are signed for one target.
pub fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
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
    Ok(Conn::with_max_frame(tls, rcli_protocol::update::MAX_SESSION_FRAME))
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
/// Resources are sampled every 5 s whether connected or not; samples taken offline are backfilled on reconnect.
pub async fn run(server: ServerEndpoint, creds: Credentials, platform: Arc<dyn Platform>) {
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
    let mut backoff = Duration::from_secs(1);
    loop {
        match connect(&server, Some(&creds)).await {
            Ok(conn) => {
                backoff = Duration::from_secs(1);
                if let Err(e) = session(conn, &server, &platform, &executor, &backlog).await {
                    tracing::warn!("connection to server lost: {e}");
                }
            }
            Err(e) => tracing::warn!("cannot connect to server: {e}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
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
    server: &ServerEndpoint,
    platform_arc: &Arc<dyn Platform>,
    executor: &Arc<Mutex<Executor>>,
    backlog: &Backlog,
) -> anyhow::Result<()> {
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
            msg = conn.recv::<ServerMessage>() => {
                match msg?.and_then(|m| m.msg) {
                    None => anyhow::bail!("server closed the connection"),
                    Some(server_message::Msg::Error(e)) => anyhow::bail!("server refused: {}: {}", e.code, e.message),
                    Some(server_message::Msg::Act(req)) => {
                        // The Executor's lock runs requests one at a time, in arrival order.
                        let (platform, executor, replies) = (platform_arc.clone(), executor.clone(), replies.clone());
                        tokio::task::spawn_blocking(move || {
                            let result = executor.lock().unwrap().act(platform.as_ref(), &req);
                            let _ = replies.send(agent_message::Msg::ActResult(result));
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
                    Some(server_message::Msg::Update(update)) => {
                        // shortcut: installs inline, blocking heartbeats; fine while installs take well under 30 s.
                        match install(server, platform, &update) {
                            // Reconnect so the Server sees the new version (real adapters restart instead).
                            Ok(()) => return Ok(()),
                            Err(e) => {
                                tracing::warn!("rejected Agent update {}: {e:#}", update.version);
                                let failed = UpdateFailed { version: update.version, reason: format!("{e:#}") };
                                conn.send(&send(agent_message::Msg::UpdateFailed(failed))).await?;
                            }
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

/// Verifies `update` against the configured update key and target, then installs it.
fn install(server: &ServerEndpoint, platform: &dyn Platform, update: &AgentUpdate) -> anyhow::Result<()> {
    let key = server.update_key.as_deref().ok_or_else(|| anyhow::anyhow!("no update key configured on this Agent"))?;
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
