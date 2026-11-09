//! Ticket 07: the Terminal User and root access Device Settings.

use rcli_testkit::{eventually, TestSystem};

// `Shortcut`: the ticket's own suite; the socket gate is covered in `device_identity.rs`.

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli07-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn exec_and_shell_run_as_the_terminal_user() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("terminal-user");
    let agent = sys.start_agent_trusting_socket("lab-pc", &dir).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;

    // The default is the account running the Agent (this test process).
    let running = std::env::var("USER").expect("USER");
    let (status, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "whoami" })).await;
    assert_eq!(status, rcli_testkit::StatusCode::OK, "{out}");
    assert_eq!(out["stdout"], format!("{running}\n"));

    sys.agent_rcli(&socket, &["agent", "terminal-user", "set", "alice"]).await.unwrap();

    let (_, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "whoami" })).await;
    assert_eq!(out["stdout"], "alice\n", "{out}");
    // `sudo` inside the terminal still works, as over SSH.
    let (_, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "sudo whoami" })).await;
    assert_eq!(out["stdout"], "root\n", "{out}");
}

#[tokio::test]
async fn only_root_can_change_the_terminal_user() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("terminal-user-gate");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    let out = sys.agent_rcli(&socket, &["agent", "terminal-user", "set", "alice"]).await;
    if rcli_testkit::running_as_root() {
        out.unwrap();
    } else {
        assert!(out.unwrap_err().to_string().contains("root"), "a non-root caller must be refused");
    }
    let _ = agent;
}
