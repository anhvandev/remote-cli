//! The Linux X11 adapter against the real OS. The `ignore`d tests need an X server, a session D-Bus with AT-SPI,
//! zenity and tesseract: `dbus-run-session -- xvfb-run -a cargo test -p rcli-agent --test linux -- --include-ignored`.
#![cfg(target_os = "linux")]

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use rcli_agent::{
    linux::LinuxPlatform,
    platform::{Button, Input, Platform, Account, UiElement},
};

fn me() -> Account {
    let out = Command::new("id").arg("-un").output().unwrap();
    Account::User(String::from_utf8_lossy(&out.stdout).trim().into())
}

#[test]
fn shell_returns_exit_code_stdout_and_stderr() {
    let out = LinuxPlatform::new("pc").unwrap().shell("echo out; echo err >&2; exit 3", me(), Duration::from_secs(5)).unwrap();
    assert_eq!((out.exit_code, out.stdout.as_str(), out.stderr.as_str(), out.timed_out), (3, "out\n", "err\n", false));
}

#[test]
fn shell_timeout_kills_the_command_and_keeps_its_output() {
    let start = Instant::now();
    let out = LinuxPlatform::new("pc").unwrap().shell("echo started; sleep 30", me(), Duration::from_secs(1)).unwrap();
    assert!(out.timed_out, "{out:?}");
    assert_eq!(out.stdout, "started\n");
    assert!(start.elapsed() < Duration::from_secs(3), "took {:?}", start.elapsed());
}

#[test]
fn pty_is_a_resizable_terminal_that_reports_the_exit_code() {
    let mut pty = LinuxPlatform::new("pc").unwrap().pty(me(), 80, 24).unwrap();
    pty.control.resize(100, 30).unwrap();
    pty.input.write_all(b"stty size\rexit 3\r").unwrap();
    let mut text = String::new();
    pty.output.read_to_string(&mut text).unwrap();
    assert!(text.contains("30 100"), "{text:?}");
    assert_eq!(pty.control.wait().unwrap(), 3);
}

#[test]
fn metrics_inventory_and_processes_describe_this_machine() {
    let p = LinuxPlatform::new("pc").unwrap();
    let m = p.metrics().unwrap();
    assert!((0.0..=100.0).contains(&m.cpu_percent.unwrap()), "{m:?}");
    assert!(m.ram_used_bytes.unwrap() > 0 && m.uptime_secs.is_some(), "{m:?}");
    let i = p.inventory().unwrap();
    assert!(!i.os.is_empty() && !i.hostname.is_empty() && i.cpu_cores > 0 && i.ram_bytes > 0, "{i:?}");
    assert!(p.processes().unwrap().iter().any(|pr| pr.pid == std::process::id()), "own process missing");
    assert_eq!(p.hostname(), "pc");
}

#[test]
fn wake_send_broadcasts() {
    LinuxPlatform::new("pc").unwrap().send_wake(rcli_agent::contract::WAKE_TEST_MAC).unwrap();
}

#[test]
#[ignore = "needs an X server"]
fn displays_have_one_primary_and_capture_at_their_size() {
    let p = LinuxPlatform::new("pc").unwrap();
    let displays = p.displays().unwrap();
    assert_eq!(displays.iter().filter(|d| d.primary).count(), 1, "{displays:?}");
    for d in displays {
        let f = p.capture(d.index).unwrap();
        assert_eq!((f.width, f.height, f.rgba.len()), (d.width, d.height, (d.width * d.height * 4) as usize));
    }
}

/// End to end on a real dialog: the AT-SPI tree finds its field and button, a click and typed text reach them,
/// and OCR reads the dialog's label.
#[test]
#[ignore = "needs an X server, AT-SPI, zenity and tesseract"]
fn click_and_type_into_a_dialog_found_through_the_accessibility_tree() {
    let p = LinuxPlatform::new("pc").unwrap();
    // Starts the accessibility bus on a bare test session, so the dialog finds it (a desktop always has one).
    let _ = p.accessibility(0);
    let mut dialog = KillOnDrop(Command::new("zenity").args(["--entry", "--text=Remote check"]).stdout(Stdio::piped()).spawn().unwrap());
    let tree = wait_for(|| p.accessibility(0).unwrap().filter(|t| has(t, "field") && has(t, "button")));
    let field = tree.iter().find(|e| e.role == "field").unwrap();
    click(&p, field);
    p.input(Input::Text("rcli-typed".into())).unwrap();

    let text = wait_for(|| Some(p.recognize_text(&p.capture(0).unwrap()).unwrap()).filter(|t| t.iter().any(|e| e.text.contains("check"))));
    assert!(text.iter().all(|e| e.role == "text" && e.width > 0), "{text:?}");

    click(&p, tree.iter().find(|e| e.role == "button" && e.text == "OK").unwrap());
    let mut typed = String::new();
    dialog.0.stdout.take().unwrap().read_to_string(&mut typed).unwrap();
    assert_eq!(typed, "rcli-typed\n");
}

/// A failed test must not leave the dialog running: it holds the test's output pipes open.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn has(tree: &[UiElement], role: &str) -> bool {
    tree.iter().any(|e| e.role == role)
}

fn click(p: &LinuxPlatform, e: &UiElement) {
    p.input(Input::Move { display: 0, x: e.x + e.width / 2, y: e.y + e.height / 2 }).unwrap();
    p.input(Input::Press { button: Button::Left, down: true }).unwrap();
    p.input(Input::Press { button: Button::Left, down: false }).unwrap();
}

fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "timed out");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Ticket 16: the installed root service cannot reach the user's or the greeter's X server, so `rcli agent
/// session` runs in that session and answers screen calls over the daemon's socket. This runs the real Linux
/// adapter on Xvfb and checks the peering: displays, capture and input reach the helper's X server.
#[test]
#[ignore = "needs an X server"]
fn the_session_helper_forwards_screen_calls_to_the_x_server() {
    use rcli_agent::{
        platform::{FakePlatform, Platform},
        macos_session::{answer, SessionPlatform},
    };
    use std::sync::Arc;

    let dir = std::env::temp_dir().join(format!("rcli-x11-session-{}-{}", std::process::id(), Instant::now().elapsed().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("session.sock");
    // The greeter accepts XTest input, so the login screen is not special (unlike the macOS one).
    let daemon = SessionPlatform::listen_with(Arc::new(FakePlatform::new("daemon")), &socket, |_| true, true).unwrap();
    let screen = LinuxPlatform::new("session").unwrap();
    let path = socket.clone();
    std::thread::spawn(move || {
        let stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let _ = answer(stream, &screen, &|| true);
    });
    let start = Instant::now();
    while daemon.helper_count() == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "the helper did not connect");
        std::thread::sleep(Duration::from_millis(20));
    }

    let displays = daemon.displays().unwrap();
    assert!(!displays.is_empty(), "the helper saw no displays");
    assert_eq!(displays.iter().filter(|d| d.primary).count(), 1, "{displays:?}");
    let frame = daemon.capture(displays[0].index).unwrap();
    assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    // XTest reaches the Xvfb screen (at the greeter too, unlike CGEvent on macOS).
    daemon.input(Input::Key("a".into())).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
