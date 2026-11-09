//! Changing the Device Password, Device ID, name and Server from the Device (`rcli agent ...` over local IPC).

use rcli_testkit::{eventually, TestSystem};

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli06-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_non_root_caller_is_refused_every_change() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("refused");
    let _agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;

    let (server, key) = (sys.agent_addr(), sys.server_key());
    let attempts: [&[&str]; 4] = [
        &["agent", "password", "set", "--password", "newpassword"],
        &["agent", "id", "set", "123456789"],
        &["agent", "name", "set", "impersonator"],
        &["agent", "server", "set", &server, &key],
    ];
    for args in attempts {
        let out = sys.agent_rcli(&socket, args).await;
        if rcli_testkit::running_as_root() {
            // Root may change the Device; refusal is the rule under test only for everyone else.
            assert!(out.is_ok(), "{args:?}: {:?}", out.err());
        } else {
            let err = out.unwrap_err().to_string();
            assert!(err.contains("root"), "{args:?}: {err}");
        }
    }
}
