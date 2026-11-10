//! Local IPC: the running Agent serves `rcli agent ...` on a Unix socket (a named pipe on Windows).
//! One JSON request per connection, one JSON reply, newline-terminated; only `tokio::io` traits are used, so a
//! pipe transport can reuse `handle`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use rcli_protocol::{agent::IdentityUpdate, password::MIN_LENGTH, server_key::ServerKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, watch},
};

use crate::{secret_store::SecretStore, settings, ServerEndpoint};

/// The Device Password's name in the Secret Store.
pub const DEVICE_PASSWORD: &str = "device_password";
/// The unlock password's name in the Secret Store.
pub const UNLOCK_PASSWORD: &str = "unlock_password";

/// How long a change may wait for the Server's answer.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(10);

/// A Device identity change that needs the Server's acknowledgement, sent on the live connection.
pub struct Change {
    pub update: IdentityUpdate,
    /// The Device ID in effect after the change, or the message to show.
    pub reply: oneshot::Sender<Result<String, String>>,
}

/// One request to the running Agent; on the wire `{"command": "<name>", ...fields}\n`, one per connection.
/// Missing text fields read as empty, so a non-root caller is told it needs root before anything else.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command")]
pub enum Request {
    /// The Device ID, connection status and connected Controllers (any user); settings and secrets for root.
    #[serde(rename = "info")]
    Info,
    /// The prompts waiting for the person at the Device (any user).
    // shortcut: the spec's IPC split lets any local user see only the ID, connection status, who is controlling and
    // disconnect. Story 27 needs the person at the Device — whoever that is, not only root — to see a prompt naming
    // who asks for root, and story 28 lets them grant or refuse it, so this listing is deliberately open. It exposes
    // requester names and pending requests to any local user; seeing a request does not grant it (accepting one
    // stays root-only, refusing is like `disconnect`), and gating the list on root would leave the prompt
    // unanswerable when nobody is an Administrator at that moment. Revisit if the split is enforced literally.
    #[serde(rename = "prompts")]
    Prompts,
    /// Answers a waiting prompt: anyone may refuse, only root may accept.
    #[serde(rename = "prompt.answer")]
    PromptAnswer {
        #[serde(default)]
        id: u64,
        #[serde(default)]
        accept: bool,
    },
    /// Ends the Controllers called `who`, or every Controller when it is empty (any user).
    #[serde(rename = "disconnect")]
    Disconnect {
        #[serde(default)]
        who: String,
    },
    #[serde(rename = "password.show")]
    PasswordShow,
    #[serde(rename = "password.set")]
    PasswordSet {
        #[serde(default)]
        password: String,
    },
    #[serde(rename = "unlock-password.set")]
    UnlockPasswordSet {
        #[serde(default)]
        password: String,
    },
    #[serde(rename = "unlock-password.clear")]
    UnlockPasswordClear,
    #[serde(rename = "id.set")]
    IdSet {
        #[serde(default)]
        device_id: String,
    },
    #[serde(rename = "name.set")]
    NameSet {
        #[serde(default)]
        name: String,
    },
    #[serde(rename = "terminal-user.set")]
    TerminalUserSet {
        #[serde(default)]
        user: String,
    },
    #[serde(rename = "root.set")]
    RootSet { root: settings::RootAccess },
    #[serde(rename = "accept.set")]
    AcceptSet { accept: settings::AcceptMode },
    /// Root for the Controller called `who`, for this session only.
    #[serde(rename = "root.grant")]
    RootGrant {
        #[serde(default)]
        who: String,
    },
    /// Another Server Address (`host:port`) and Server Key; the Agent reconnects at once.
    #[serde(rename = "server.set")]
    ServerSet {
        #[serde(default)]
        server: String,
        #[serde(default)]
        key: String,
    },
}

/// What a prompt asks the person at the Device.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptKind {
    /// Root for a terminal command or shell (ticket 07).
    Root,
    /// Let a new Controller in, in `password+accept` mode (ticket 08).
    Accept,
}

/// Prompts for the person at the Device (tickets 07/08), waiting for the GUI's answer over the IPC.
#[derive(Default)]
pub struct Prompts {
    state: Mutex<PromptState>,
}

#[derive(Default)]
struct PromptState {
    next_id: u64,
    pending: Vec<PendingPrompt>,
    answers: HashMap<u64, std::sync::mpsc::Sender<bool>>,
}

struct PendingPrompt {
    id: u64,
    kind: PromptKind,
    who: String,
    deadline: Instant,
}

impl Prompts {
    /// Publishes a prompt and waits for the person's answer, for at most `timeout`; no answer rejects (30 s rule).
    pub fn ask(&self, kind: PromptKind, who: &str, timeout: Duration) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        let id = {
            let mut state = self.state.lock().unwrap();
            state.next_id += 1;
            let id = state.next_id;
            state.pending.push(PendingPrompt { id, kind, who: who.to_owned(), deadline: Instant::now() + timeout });
            state.answers.insert(id, tx);
            id
        };
        let accepted = rx.recv_timeout(timeout).unwrap_or(false);
        self.forget(id);
        accepted
    }

    fn forget(&self, id: u64) {
        let mut state = self.state.lock().unwrap();
        state.pending.retain(|p| p.id != id);
        state.answers.remove(&id);
    }

    /// The prompts waiting for an answer right now (id, kind, who, seconds left).
    pub fn list(&self) -> Value {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        state.pending.retain(|p| p.deadline > now);
        serde_json::Value::Array(
            state
                .pending
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "kind": p.kind,
                        "who": p.who,
                        "seconds_left": p.deadline.duration_since(now).as_secs(),
                    })
                })
                .collect(),
        )
    }

    /// Answers a waiting prompt; `Err` names an unknown or already-answered one.
    pub fn answer(&self, id: u64, accept: bool) -> Result<(), String> {
        let sender = self.state.lock().unwrap().answers.remove(&id);
        match sender {
            Some(sender) => {
                self.forget(id);
                let _ = sender.send(accept);
                Ok(())
            }
            None => Err(format!("no prompt {id} is waiting for an answer")),
        }
    }
}

/// What the local callers see of the running Agent: `rcli agent …`, the GUI and scripts.
pub struct Status {
    device_id: RwLock<String>,
    /// The Server the Agent connects to; a change reconnects it without a restart.
    endpoint: watch::Sender<ServerEndpoint>,
    connected: AtomicBool,
    secrets: Arc<dyn SecretStore>,
    /// Where `rcli agent run` keeps its credentials, so a new Device ID survives a restart (None in tests).
    credentials: Mutex<Option<PathBuf>>,
    changes: mpsc::UnboundedSender<Change>,
    changes_rx: Mutex<Option<mpsc::UnboundedReceiver<Change>>>,
    /// Controllers the person at the Device disconnected, forwarded to the Server over the live connection.
    disconnects: mpsc::UnboundedSender<String>,
    disconnects_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
    next_request: AtomicU64,
    /// The Device Settings the Agent enforces; kept here so `rcli agent …` can change them live.
    settings: Arc<settings::Handle>,
    /// Root granted to the current session, by the person at the Device. Cleared when the Agent reconnects or that
    /// Controller is disconnected.
    grant: Arc<crate::Grant>,
    /// Controllers currently using this Device, so `rcli agent info` / `disconnect` can see and end them.
    controllers: Arc<crate::Controllers>,
    /// Prompts for the person at the Device (tickets 07/08), shown and answered by the GUI over the IPC.
    prompts: Arc<Prompts>,
}

impl Status {
    pub fn new(device_id: &str, endpoint: ServerEndpoint, secrets: Arc<dyn SecretStore>) -> Self {
        let (changes, changes_rx) = mpsc::unbounded_channel();
        let (disconnects, disconnects_rx) = mpsc::unbounded_channel();
        Self {
            device_id: RwLock::new(device_id.into()),
            endpoint: watch::channel(endpoint).0,
            connected: AtomicBool::new(false),
            secrets,
            credentials: Mutex::new(None),
            changes,
            changes_rx: Mutex::new(Some(changes_rx)),
            disconnects,
            disconnects_rx: Mutex::new(Some(disconnects_rx)),
            next_request: AtomicU64::new(0),
            settings: Arc::new(settings::Handle::new()),
            grant: Arc::new(crate::Grant::default()),
            controllers: Arc::new(crate::Controllers::default()),
            prompts: Arc::new(Prompts::default()),
        }
    }

    /// Where Device Settings are saved (the Agent's state directory); they apply to the running Agent at once.
    pub fn set_settings_dir(&self, dir: std::path::PathBuf) {
        self.settings.set_dir(dir);
    }

    pub fn settings(&self) -> Arc<settings::Handle> {
        self.settings.clone()
    }

    /// The Secret Store, for the Server's `unlock` (the password is read here, never sent to the Server).
    pub fn secrets(&self) -> Arc<dyn SecretStore> {
        self.secrets.clone()
    }

    /// The session root grant: shared with `run`, cleared whenever the connection is re-established.
    pub fn grant(&self) -> Arc<crate::Grant> {
        self.grant.clone()
    }

    /// The connected Controllers, shared with `run` (so `info` / `disconnect` see the live set).
    pub fn controllers(&self) -> Arc<crate::Controllers> {
        self.controllers.clone()
    }

    /// The prompts waiting for the person at the Device; the GUI lists and answers them here.
    pub fn prompts(&self) -> Arc<Prompts> {
        self.prompts.clone()
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn device_id(&self) -> String {
        self.device_id.read().unwrap().clone()
    }

    pub fn endpoint(&self) -> ServerEndpoint {
        self.endpoint.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ServerEndpoint> {
        self.endpoint.subscribe()
    }

    /// Points the Agent at another Server: the running session reconnects with the new values.
    pub fn set_endpoint(&self, endpoint: ServerEndpoint) {
        self.endpoint.send_replace(endpoint);
    }

    /// Tells the Agent to rewrite `credentials.json` (so a new Device ID survives a restart).
    pub fn set_credentials_path(&self, path: PathBuf) {
        *self.credentials.lock().unwrap() = Some(path);
    }

    /// The change receiver, taken once by `run`.
    pub fn take_changes(&self) -> Option<mpsc::UnboundedReceiver<Change>> {
        self.changes_rx.lock().unwrap().take()
    }

    /// The disconnect receiver, taken once by `run`.
    pub fn take_disconnects(&self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.disconnects_rx.lock().unwrap().take()
    }

    /// Asks the Server (over the live connection) to end `controller`'s Control Lock and sessions.
    pub fn request_disconnect(&self, controller: &str) {
        let _ = self.disconnects.send(controller.to_owned());
    }

    /// Stores a Device Password the Server set from the Dashboard.
    pub fn store_password(&self, password: &str) -> anyhow::Result<()> {
        self.secrets.set(DEVICE_PASSWORD, password)
    }

    /// The unlock password set at this Device; None = none set.
    pub fn unlock_password(&self) -> anyhow::Result<Option<String>> {
        self.secrets.get(UNLOCK_PASSWORD)
    }

    pub fn set_device_id(&self, device_id: &str) {
        *self.device_id.write().unwrap() = device_id.to_owned();
        let path = self.credentials.lock().unwrap().clone();
        let Some(path) = path else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        let Ok(mut creds) = serde_json::from_slice::<crate::Credentials>(&bytes) else { return };
        creds.device_id = device_id.to_owned();
        if let Ok(bytes) = serde_json::to_vec(&creds) {
            let _ = std::fs::write(&path, bytes);
        }
    }

    /// Sends a change to the Server and waits for its answer; `Err` is the message to show.
    async fn identity(&self, password: Option<&str>, device_id: Option<&str>, name: Option<&str>) -> Result<String, String> {
        if !self.connected.load(Ordering::Relaxed) {
            return Err("the Agent is not connected to the Server; try again once it is Online".into());
        }
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        let update = IdentityUpdate {
            request_id,
            device_password: password.unwrap_or_default().into(),
            device_id: device_id.unwrap_or_default().into(),
            name: name.unwrap_or_default().into(),
        };
        let (reply, answer) = oneshot::channel();
        if self.changes.send(Change { update, reply }).is_err() {
            return Err("the Agent is not running".into());
        }
        match tokio::time::timeout(CHANGE_TIMEOUT, answer).await {
            Ok(Ok(result)) => result,
            // shortcut: a connection lost after the Server applied the change also lands here; re-read `rcli agent info`
            // to see the Device ID. Add an idempotent re-check if that race ever shows up.
            Ok(Err(_)) | Err(_) => Err("the Agent did not get an answer from the Server; check `rcli agent info`".into()),
        }
    }

    fn info(&self, is_root: bool) -> Value {
        let mut info = json!({
            "device_id": self.device_id(),
            "server": self.endpoint.borrow().addr,
            "connected": self.connected.load(Ordering::Relaxed),
            "controllers": self.controllers.list(),
        });
        // Everything outside the ID, status and controllers is root-only: the Device Settings and the secrets (the
        // GUI reveals them elevated).
        if is_root {
            info["settings"] = json!(self.settings.get());
            info["server_key"] = self.endpoint.borrow().server_key.clone().into();
            info["unlock_password"] = match self.unlock_password() {
                Ok(Some(password)) => password.into(),
                _ => Value::Null,
            };
        }
        info
    }
}

/// Where the Agent listens when no other path is given.
pub fn default_socket() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/var/run/rcli/agent.sock");
    #[cfg(windows)]
    return PathBuf::from(r"\\.\pipe\rcli-agent");
    #[cfg(not(any(target_os = "macos", windows)))]
    PathBuf::from("/run/rcli/agent.sock")
}

#[cfg(unix)]
pub async fn serve(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    // Only root may read the Device Password or change the Device; `info` is for any local user.
    serve_with(path, status, |stream| stream.peer_cred().is_ok_and(|c| c.uid() == 0)).await
}

/// Tests run unprivileged: this stands in for "the caller is root", nothing else.
#[cfg(unix)]
pub async fn serve_trusting_all(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    serve_with(path, status, |_| true).await
}

#[cfg(unix)]
async fn serve_with(
    path: PathBuf,
    status: Arc<Status>,
    is_root: impl Fn(&tokio::net::UnixStream) -> bool + Send + Sync + 'static,
) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    // Any local user may connect (info, disconnect); the peer credential, not the file mode, gates the rest.
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
    let is_root = Arc::new(is_root);
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let status = status.clone();
        let is_root = is_root(&stream);
        tokio::spawn(async move {
            if let Err(e) = handle(stream, status, is_root).await {
                tracing::debug!("IPC client ended: {e}");
            }
        });
    }
}

/// Windows: a named pipe any local user may open (info, disconnect); an elevated caller counts as root.
// shortcut: a busy pipe is not retried by `send`; one request per connection keeps that rare.
#[cfg(windows)]
pub async fn serve(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    serve_windows(path, status, false).await
}

/// Tests run unprivileged: this stands in for "the caller is root", nothing else.
#[cfg(windows)]
pub async fn serve_trusting_all(path: PathBuf, status: Arc<Status>) -> anyhow::Result<()> {
    serve_windows(path, status, true).await
}

#[cfg(windows)]
async fn serve_windows(path: PathBuf, status: Arc<Status>, trust_all: bool) -> anyhow::Result<()> {
    let mut server = pipe_server(&path, true)?;
    loop {
        server.connect().await?;
        let client = std::mem::replace(&mut server, pipe_server(&path, false)?);
        let is_root = trust_all || client_is_elevated(&client);
        let status = status.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(client, status, is_root).await {
                tracing::debug!("IPC client ended: {e}");
            }
        });
    }
}

/// One pipe instance. The first refuses to start if another process already owns the name (pipe squatting).
#[cfg(windows)]
fn pipe_server(path: &Path, first: bool) -> anyhow::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use windows::Win32::Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    };
    // SYSTEM and Administrators: full access; Everyone: read and write, so any local user can send a request. The
    // helper pipe has the same callers, so both use one DACL (crate::windows_session::PIPE_DACL).
    let dacl: Vec<u16> = crate::windows_session::PIPE_DACL.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(windows::core::PCWSTR(dacl.as_ptr()), SDDL_REVISION_1, &mut sd, None)?;
    }
    let mut attrs = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
    let server = unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .create_with_security_attributes_raw(path, &mut attrs as *mut _ as *mut _)
    };
    unsafe {
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(sd.0)));
    }
    Ok(server?)
}

/// Whether the process at the other end of `pipe` runs elevated (an Administrator shell or SYSTEM).
// shortcut: a user-session Agent cannot open an elevated caller's token, so there nobody counts as root; the
// installed SYSTEM service (ticket 22) can.
#[cfg(windows)]
fn client_is_elevated(pipe: &tokio::net::windows::named_pipe::NamedPipeServer) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            Pipes::GetNamedPipeClientProcessId,
            Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    unsafe {
        let mut pid = 0;
        if GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut pid).is_err() {
            return false;
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
        let _ = CloseHandle(process);
        if !opened {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0;
        let size = size_of::<TOKEN_ELEVATION>() as u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut elevation as *mut _ as *mut _), size, &mut len).is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Answers one request on `conn`. `is_root` is the caller's peer credential.
async fn handle<S>(conn: S, status: Arc<Status>, is_root: bool) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(conn);
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let request: Value = serde_json::from_str(&line)?;
    let reply = dispatch(&status, &request, is_root).await;
    write.write_all(format!("{reply}\n").as_bytes()).await?;
    write.flush().await?;
    Ok(())
}

/// The reply to one request. Any local user may read the Device ID and status; everything else is root-only.
async fn dispatch(status: &Status, request: &Value, is_root: bool) -> Value {
    let parsed = match serde_json::from_value::<Request>(request.clone()) {
        Ok(parsed) => parsed,
        Err(e) => return json!({ "error": format!("bad request: {e}") }),
    };
    match parsed {
        Request::Info => status.info(is_root),
        // Who is asking is status-adjacent: any local user may list the waiting prompts (see the `Prompts` variant).
        Request::Prompts => status.prompts().list(),
        Request::PromptAnswer { id, accept } => {
            // Anyone at the Device may refuse a prompt (like `disconnect`); only root may accept one,
            // so a local process cannot grant root or let a Controller in behind the person's back.
            if accept && !is_root {
                return json!({ "error": "accepting a prompt requires root (run `sudo rcli agent prompt answer …`)" });
            }
            match status.prompts().answer(id, accept) {
                Ok(()) => json!({ "message": format!("prompt {id} {}", if accept { "accepted" } else { "rejected" }) }),
                Err(e) => json!({ "error": e }),
            }
        }
        Request::Disconnect { who } => {
            // Any local user may end a Controller: the person at the Device has the last word, no root needed.
            let who = who.trim();
            let removed = if who.is_empty() { status.controllers().clear() } else { status.controllers().remove(who) };
            for c in &removed {
                status.grant().revoke(c);
                status.request_disconnect(c);
            }
            let message =
                if removed.is_empty() { "no matching Controller was connected".to_owned() } else { format!("disconnected {}", removed.join(", ")) };
            json!({ "disconnected": removed, "message": message })
        }
        _ if !is_root => {
            let command = request["command"].as_str().unwrap_or_default().replace('.', " ");
            json!({ "error": format!("this requires root (run `sudo rcli agent {command}`)") })
        }
        Request::PasswordShow => match status.secrets.get(DEVICE_PASSWORD) {
            Ok(Some(password)) => json!({ "password": password }),
            Ok(None) => json!({ "error": "no Device Password is stored" }),
            Err(e) => json!({ "error": format!("{e:#}") }),
        },
        Request::UnlockPasswordSet { password } => {
            if password.is_empty() {
                return json!({ "error": "the unlock password must not be empty" });
            }
            match status.secrets.set(UNLOCK_PASSWORD, &password) {
                Ok(()) => json!({ "message": "the unlock password was set" }),
                Err(e) => json!({ "error": format!("cannot store the unlock password: {e:#}") }),
            }
        }
        Request::UnlockPasswordClear => match status.secrets.remove(UNLOCK_PASSWORD) {
            Ok(()) => json!({ "message": "the unlock password was cleared" }),
            Err(e) => json!({ "error": format!("cannot clear the unlock password: {e:#}") }),
        },
        Request::PasswordSet { password } => {
            if password.chars().count() < MIN_LENGTH {
                return json!({ "error": format!("the Device Password must be at least {MIN_LENGTH} characters") });
            }
            match status.identity(Some(&password), None, None).await {
                Ok(_) => match status.secrets.set(DEVICE_PASSWORD, &password) {
                    Ok(()) => json!({ "message": "the Device Password was changed" }),
                    Err(e) => json!({ "error": format!("the Server changed it, but the Device could not store it: {e:#}") }),
                },
                Err(e) => json!({ "error": e }),
            }
        }
        Request::IdSet { device_id } => match status.identity(None, Some(&device_id), None).await {
            Ok(id) => json!({ "device_id": id, "message": format!("the Device ID is now {id}") }),
            Err(e) => json!({ "error": e }),
        },
        Request::NameSet { name } => {
            // Empty means "unchanged" on the wire, so a blank name is refused here.
            let name = name.trim();
            if name.is_empty() || name.len() > 255 {
                return json!({ "error": "name must be 1-255 characters" });
            }
            match status.identity(None, None, Some(name)).await {
                Ok(_) => json!({ "message": "the Device name was changed" }),
                Err(e) => json!({ "error": e }),
            }
        }
        Request::TerminalUserSet { user } => {
            let user = user.trim().to_owned();
            if user.is_empty() || user.len() > 64 || user.chars().any(|c| c.is_whitespace() || c == ':') {
                return json!({ "error": "the Terminal User must be a local account name (1-64 characters, no spaces)" });
            }
            match status.settings().update(|s| s.terminal_user = user.clone()) {
                Ok(s) => json!({ "terminal_user": s.terminal_user, "message": format!("terminal commands and shells now run as {user}") }),
                Err(e) => json!({ "error": format!("cannot save the Device Settings: {e:#}") }),
            }
        }
        Request::RootSet { root } => match status.settings().update(|s| s.root = root) {
            Ok(s) => json!({ "root": s.root.name(), "message": format!("root access is now {}", s.root.name()) }),
            Err(e) => json!({ "error": format!("cannot save the Device Settings: {e:#}") }),
        },
        Request::AcceptSet { accept } => match status.settings().update(|s| s.accept = accept) {
            Ok(s) => json!({ "accept": s.accept.name(), "message": format!("new connections now ask for {}", s.accept.name()) }),
            Err(e) => json!({ "error": format!("cannot save the Device Settings: {e:#}") }),
        },
        Request::RootGrant { who } => {
            // The person at the Device: allow root for the Controller named here, for this session only.
            let who = who.trim();
            if who.is_empty() {
                return json!({ "error": "say who the root grant is for" });
            }
            status.grant().grant(who);
            json!({ "message": format!("{who} may use root for this session") })
        }
        Request::ServerSet { server, key } => {
            let (addr, key) = (server.trim(), key.trim());
            if addr.is_empty() || ServerKey::parse(key).is_none() {
                return json!({ "error": "give the Server Address (host:port) and a Server Key rk_<fingerprint>_<secret>" });
            }
            status.set_endpoint(ServerEndpoint { addr: addr.into(), server_key: key.into() });
            json!({ "server": addr, "message": format!("the Agent is reconnecting to {addr}") })
        }
    }
}

/// Sends one request to the Agent at `path` and returns its reply (an `error` reply is returned as `Err`).
pub async fn send(path: &Path, request: &Request) -> anyhow::Result<Value> {
    let request = serde_json::to_string(request)?;
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(path).await;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(path);
    let stream = stream.map_err(|e| anyhow::anyhow!("cannot reach the Agent at {}: {e}", path.display()))?;
    let (read, mut write) = tokio::io::split(stream);
    write.write_all(format!("{request}\n").as_bytes()).await?;
    write.flush().await?;
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let reply: Value = serde_json::from_str(&line)?;
    match reply["error"].as_str() {
        Some(e) => anyhow::bail!("{e}"),
        None => Ok(reply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_store::FileSecretStore;

    #[test]
    fn only_root_may_show_or_change_the_device() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        store.set(DEVICE_PASSWORD, "abcd2345ef").unwrap();
        let status = Status::new("123456789", ServerEndpoint { addr: "localhost:1".into(), server_key: String::new() }, Arc::new(store));

        let show = json!({ "command": "password.show" });
        let block = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(block.block_on(dispatch(&status, &show, true))["password"], "abcd2345ef");
        for command in ["password.show", "id.set", "name.set", "server.set", "password.set"] {
            let reply = block.block_on(dispatch(&status, &json!({ "command": command }), false));
            assert!(reply["error"].as_str().unwrap().contains("root"), "{command}: {reply}");
        }
        assert_eq!(block.block_on(dispatch(&status, &json!({ "command": "info" }), false))["device_id"], "123456789");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_change_is_at_least_eight_characters() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-short-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        let status = Status::new("123456789", ServerEndpoint { addr: "localhost:1".into(), server_key: String::new() }, Arc::new(store));

        let reply = tokio::runtime::Runtime::new().unwrap().block_on(dispatch(&status, &json!({ "command": "password.set", "password": "short" }), true));
        assert!(reply["error"].as_str().unwrap().contains("8 characters"), "{reply}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompts_are_listed_answered_or_rejected_on_timeout() {
        let prompts = Arc::new(Prompts::default());
        let answerer = {
            let prompts = prompts.clone();
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    let listed = prompts.list();
                    if let Some(p) = listed.as_array().and_then(|a| a.first()) {
                        prompts.answer(p["id"].as_u64().unwrap(), true).unwrap();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                panic!("the prompt was never listed");
            })
        };
        assert!(prompts.ask(PromptKind::Root, "alice", Duration::from_secs(5)));
        answerer.join().unwrap();
        assert!(prompts.list().as_array().unwrap().is_empty(), "{:?}", prompts.list());
        // Nobody answers: the prompt auto-rejects when its time runs out.
        assert!(!prompts.ask(PromptKind::Accept, "dev-laptop (10.0.0.5)", Duration::from_millis(50)));
        assert!(prompts.answer(999, true).is_err());
    }

    #[test]
    fn anyone_may_refuse_a_prompt_but_only_root_accepts_and_sees_secrets() {
        let dir = std::env::temp_dir().join(format!("rcli-ipc-prompts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        store.set(UNLOCK_PASSWORD, "at-the-lock-screen").unwrap();
        let status = Status::new(
            "123456789",
            ServerEndpoint { addr: "localhost:1".into(), server_key: "rk_deadbeef_secret".into() },
            Arc::new(store),
        );
        let block = tokio::runtime::Runtime::new().unwrap();
        let prompts = status.prompts();

        let waiting = |kind, who| {
            let prompts = prompts.clone();
            std::thread::spawn(move || prompts.ask(kind, who, Duration::from_secs(5)))
        };
        let next_id = |prompts: &Prompts| loop {
            let listed = prompts.list();
            if let Some(p) = listed.as_array().and_then(|a| a.first()) {
                break p["id"].as_u64().unwrap();
            }
            std::thread::sleep(Duration::from_millis(5));
        };

        let asker = waiting(PromptKind::Root, "alice");
        let id = next_id(&prompts);
        let accept = block.block_on(dispatch(&status, &json!({ "command": "prompt.answer", "id": id, "accept": true }), false));
        assert!(accept["error"].as_str().unwrap().contains("root"), "{accept}");
        let reject = block.block_on(dispatch(&status, &json!({ "command": "prompt.answer", "id": id, "accept": false }), false));
        assert!(reject["message"].as_str().is_some(), "{reject}");
        assert!(!asker.join().unwrap());

        let asker = waiting(PromptKind::Accept, "dev-laptop (10.0.0.5)");
        let id = next_id(&prompts);
        let accept = block.block_on(dispatch(&status, &json!({ "command": "prompt.answer", "id": id, "accept": true }), true));
        assert!(accept["message"].as_str().is_some(), "{accept}");
        assert!(asker.join().unwrap());

        // The Device Settings, the Server Key and the unlock password appear in `info` only for root.
        let as_user = block.block_on(dispatch(&status, &json!({ "command": "info" }), false));
        assert!(
            as_user.get("settings").is_none() && as_user.get("server_key").is_none() && as_user.get("unlock_password").is_none(),
            "{as_user}"
        );
        let as_root = block.block_on(dispatch(&status, &json!({ "command": "info" }), true));
        assert!(as_root.get("settings").is_some(), "{as_root}");
        assert_eq!(as_root["server_key"], "rk_deadbeef_secret");
        assert_eq!(as_root["unlock_password"], "at-the-lock-screen");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
