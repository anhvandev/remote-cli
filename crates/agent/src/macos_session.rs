//! The Unix transport for the Agent service ↔ session helper split (see `session.rs`): macOS ticket 24 and Linux ticket
//! 16. Both installed Agents are root services (a macOS LaunchDaemon, a Linux systemd unit), and a root daemon
//! cannot reach the window server of the logged-in person. A helper in each GUI session (the macOS LoginWindow and
//! Aqua sessions, the Linux seat0 X11 session the Agent service spawns) connects back over a local Unix socket and
//! does the screen work; the Agent service sends those calls to the helper whose session is on the console.
//!
//! The module keeps the macOS name because the CLI, the contract binary and the LaunchAgent refer to it; on Linux
//! `linux::session` starts the helper and reuses this transport.

use std::{
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    sync::Arc,
    time::Duration,
};

use crate::platform::Platform;

pub use crate::session::LOGIN_WINDOW_INPUT;
pub use crate::session::{Helper, SessionPlatform};

impl SessionPlatform {
    /// Listens on `socket` for the macOS session helpers run by root (the login window) or by the console user.
    #[cfg(target_os = "macos")]
    pub fn listen(inner: Arc<dyn Platform>, socket: &Path) -> anyhow::Result<Arc<Self>> {
        Self::listen_with(inner, socket, |uid| uid == 0 || console_uid() == Some(uid), false)
    }

    /// `listen` with the peer check `trust(uid)` given (tests run as an ordinary user, and the Linux Agent service trusts
    /// the uid it spawned the helper as). `allow_login_input` lets a greeter helper accept input; macOS must not
    /// (`LOGIN_WINDOW_INPUT`), Linux must.
    pub fn listen_with(
        inner: Arc<dyn Platform>,
        socket: &Path,
        trust: impl Fn(u32) -> bool + Send + 'static,
        allow_login_input: bool,
    ) -> anyhow::Result<Arc<Self>> {
        if let Some(dir) = socket.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let _ = std::fs::remove_file(socket);
        let listener = UnixListener::bind(socket)?;
        // The user's helper must be able to connect; the peer uid, not the file mode, decides who is kept.
        std::fs::set_permissions(socket, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
        let platform = Arc::new(SessionPlatform::new(inner, allow_login_input));
        let helpers = platform.helpers();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !peer_uid(&stream).is_some_and(&trust) {
                    continue;
                }
                // A hung helper must not hang the Agent: it is dropped like one that went away.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let Ok(writer) = stream.try_clone() else {
                    continue;
                };
                helpers.lock().unwrap().push(Helper::new(Box::new(stream), Box::new(writer)));
            }
        });
        Ok(platform)
    }
}

/// The session helper (`rcli agent session`): connects to the Agent service at `socket` and answers its screen calls from
/// `platform`, reconnecting whenever the Agent service restarts. `on_console` says whether this is the one shown.
pub fn run_helper(socket: &Path, platform: &dyn Platform, on_console: &dyn Fn() -> bool) -> ! {
    loop {
        if let Ok(stream) = UnixStream::connect(socket) {
            if let Err(e) = answer(stream, platform, on_console) {
                tracing::debug!("session helper connection ended: {e:#}");
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Answers the Agent service's requests on one connection until it closes.
pub fn answer(stream: UnixStream, platform: &dyn Platform, on_console: &dyn Fn() -> bool) -> anyhow::Result<()> {
    let writer = stream.try_clone()?;
    crate::session::answer(Box::new(stream), Box::new(writer), platform, on_console)
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid) = (0, 0);
        (unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ok = unsafe {
            libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) == 0
        };
        ok.then_some(cred.uid)
    }
}

/// macOS: the user logged in on the console: the owner of `/dev/console` (root at the login window).
#[cfg(target_os = "macos")]
fn console_uid() -> Option<u32> {
    std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/dev/console").ok()?).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{FakePlatform, Input, PowerAction, SessionState};

    /// A service-side SessionPlatform with one helper serving `screen` (a FakePlatform session) over a real socket.
    fn service_with_helper(screen: FakePlatform, on_console: bool) -> (Arc<SessionPlatform>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rcli-session-{}-{}", std::process::id(), rand_suffix()));
        let socket = dir.join("session.sock");
        let service = SessionPlatform::listen_with(Arc::new(FakePlatform::new("mac")), &socket, |_| true, false).unwrap();
        let path = socket.clone();
        std::thread::spawn(move || {
            let stream = UnixStream::connect(&path).unwrap();
            let _ = answer(stream, &screen, &move || on_console);
        });
        for _ in 0..200 {
            if !service.helpers().lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (service, dir)
    }

    /// Unique per test, even when several start in the same nanosecond: a shared socket path makes one test's
    /// helper connect to another test's service.
    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
            + NEXT.fetch_add(1, Ordering::Relaxed) * 1_000_000_007
    }

    #[test]
    fn screen_calls_reach_the_session_on_the_console() {
        let screen = FakePlatform::new("session");
        screen.set_session_state(SessionState::Locked);
        let (service, dir) = service_with_helper(screen.clone(), true);
        assert_eq!(service.session_state().unwrap(), SessionState::Locked);
        assert_eq!(service.displays().unwrap(), screen.displays().unwrap());
        assert_eq!(service.capture(0).unwrap(), screen.capture(0).unwrap());
        service.input(Input::Key("a".into())).unwrap();
        assert_eq!(screen.inputs(), vec![Input::Key("a".into())], "the key reached the lock screen session");
        service.power(PowerAction::Lock).unwrap();
        assert_eq!(screen.power_actions(), vec![PowerAction::Lock], "the session locked itself");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn at_the_login_window_capture_works_and_input_is_unavailable() {
        let screen = FakePlatform::new("loginwindow");
        screen.set_session_state(SessionState::LoginScreen);
        let (service, dir) = service_with_helper(screen.clone(), true);
        assert_eq!(service.session_state().unwrap(), SessionState::LoginScreen);
        assert!(service.capture(0).is_ok());
        let err = service.input(Input::Key("a".into())).unwrap_err();
        assert_eq!(err.to_string(), "input unavailable at macOS login window");
        assert!(screen.inputs().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_a_session_on_the_console_it_reports_the_login_window_and_cannot_capture() {
        let (service, dir) = service_with_helper(FakePlatform::new("switched-out user"), false);
        assert_eq!(service.session_state().unwrap(), SessionState::LoginScreen);
        assert!(service.capture(0).unwrap_err().to_string().contains("no GUI session"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
