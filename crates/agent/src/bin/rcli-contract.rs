//! Runs the Platform contract suite on this machine. Prints the plan first, since the operator cannot read
//! a terminal at the lock or login screen; each step then gets a fixed time before the checks continue.
//! Ends with a markdown results table to paste into the README of the adapter's ticket.

use std::time::Duration;

use rcli_agent::contract;

const STEP_TIME: Duration = Duration::from_secs(20);

fn main() {
    #[cfg(target_os = "linux")]
    let platform: std::sync::Arc<dyn rcli_agent::platform::Platform> = {
        let linux = std::sync::Arc::new(rcli_agent::linux::LinuxPlatform::new("contract").expect("Linux adapter"));
        // `--session-socket <path>` as root: the installed service, screen work done by the session helper.
        let args: Vec<String> = std::env::args().collect();
        match args.iter().position(|a| a == "--session-socket").and_then(|i| args.get(i + 1)) {
            Some(socket) => rcli_agent::linux::session::console(linux, socket.as_ref()).expect("session socket"),
            None => linux,
        }
    };
    #[cfg(target_os = "macos")]
    let platform: std::sync::Arc<dyn rcli_agent::platform::Platform> = {
        let mac = std::sync::Arc::new(rcli_agent::macos::MacPlatform::new("contract").expect("macOS adapter"));
        // `--session-socket <path>`: run as the installed root daemon does, the screen work done by the session
        // helpers (the LaunchAgent) that connect there (ticket 24: lock screen and login window).
        let args: Vec<String> = std::env::args().collect();
        match args.iter().position(|a| a == "--session-socket").and_then(|i| args.get(i + 1)) {
            Some(socket) => rcli_agent::macos_session::SessionPlatform::listen(mac, socket.as_ref()).expect("session socket"),
            None => mac,
        }
    };
    #[cfg(windows)]
    let platform: std::sync::Arc<dyn rcli_agent::platform::Platform> =
        std::sync::Arc::new(rcli_agent::windows_platform::WindowsPlatform::new("contract").expect("Windows adapter"));
    // shortcut: no adapter for other OSes; they run on the FakePlatform.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let platform: std::sync::Arc<dyn rcli_agent::platform::Platform> =
        std::sync::Arc::new(rcli_agent::platform::FakePlatform::new("contract"));
    eprintln!("Platform contract suite. Each step below gets {} s:", STEP_TIME.as_secs());
    use contract::Step::*;
    for step in [FocusTextField, Lock, Unlock, LogOut, LogIn, Unlock] {
        eprintln!("  - {}", step.instruction());
    }
    eprintln!("(The suite locks the screen itself before the last step.)\n");
    let report = contract::run(platform.as_ref(), &mut |step| {
        eprintln!("NOW: {}", step.instruction());
        std::thread::sleep(STEP_TIME);
    });
    print!("{}", report.table());
    std::process::exit(if report.failures().is_empty() { 0 } else { 1 });
}
