//! Ticket 08: accept each connection, see who is controlling, disconnect.

use rcli_testkit::{eventually, TestSystem};

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli08-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn only_root_can_change_accept_mode() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("accept-gate");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    let out = sys.agent_rcli(&socket, &["agent", "accept", "password+accept"]).await;
    if rcli_testkit::running_as_root() {
        out.unwrap();
    } else {
        assert!(out.unwrap_err().to_string().contains("root"), "a non-root caller must be refused");
    }
    let _ = agent;
}

#[tokio::test]
async fn accept_mode_survives_a_settings_reload() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("accept-persist");
    let agent = sys.start_agent_trusting_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    sys.agent_rcli(&socket, &["agent", "accept", "password+accept"]).await.unwrap();

    let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["accept"], "password+accept");
    let _ = agent;
}
