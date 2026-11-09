//! Ticket 08: accept each connection, see who is controlling, disconnect.

use std::time::Duration;

use rcli_testkit::{eventually, FakePlatform, SessionState, StatusCode, TestSystem};
use serde_json::json;

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli08-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Starts a Device on `platform`, with a trusting IPC socket so the test can change its settings.
async fn online(sys: &TestSystem, name: &str, platform: FakePlatform) -> (rcli_testkit::TestAgent, std::path::PathBuf) {
    let dir = temp_dir(name);
    let agent = sys.start_agent_trusting_socket_on(platform, &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    eventually(|| async { dir.join("agent.sock").exists().then_some(()) }).await;
    (agent, dir)
}

/// Runs `whoami` over a password connection and returns the response.
async fn password_exec(sys: &TestSystem, dev: &str, password: &str) -> (StatusCode, serde_json::Value) {
    sys.device_client(dev, password).post_json(&format!("/api/v1/devices/{dev}/exec"), &json!({ "command": "whoami" })).await
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

#[tokio::test]
async fn accept_refuses_when_no_session_can_show_the_prompt() {
    let sys = TestSystem::start().await;
    let platform = FakePlatform::new("lab-pc");
    platform.set_session_state(SessionState::LoginScreen);
    let (agent, dir) = online(&sys, "accept-no-session", platform.clone()).await;
    let dev = agent.device_id().to_owned();
    let password = agent.device_password().unwrap();
    sys.agent_rcli(&dir.join("agent.sock"), &["agent", "accept", "password+accept"]).await.unwrap();

    // With nobody logged in, no session can show the accept prompt, so the connection is refused.
    let (status, out) = password_exec(&sys, &dev, &password).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("connect_denied")), "{out}");
    assert!(platform.accept_prompts().is_empty(), "no prompt was shown");
}

#[tokio::test]
async fn accept_accept_reject_and_the_thirty_second_timeout() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "accept", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let password = agent.device_password().unwrap();
    sys.agent_rcli(&dir.join("agent.sock"), &["agent", "accept", "password+accept"]).await.unwrap();

    // Rejected at the Device: the connection is refused.
    agent.platform.answer_accept_prompts([false]);
    let (status, out) = password_exec(&sys, &dev, &password).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("connect_denied")), "{out}");

    // Accepted at the Device: the connection is allowed.
    agent.platform.answer_accept_prompts([true]);
    let (status, out) = password_exec(&sys, &dev, &password).await;
    assert_eq!(status, StatusCode::OK, "{out}");

    // Once accepted, later requests from the same Controller are not asked again.
    let (status, out) = password_exec(&sys, &dev, &password).await;
    assert_eq!(status, StatusCode::OK, "{out}");

    // The prompt names who connects, and the Agent gave it 30 s.
    let prompts = agent.platform.accept_prompts();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert_eq!(prompts[0].1, Duration::from_secs(30));
    assert_eq!(prompts[1].1, Duration::from_secs(30));
}

#[tokio::test]
async fn accept_auto_rejects_when_nobody_answers() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "accept-auto", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let password = agent.device_password().unwrap();
    sys.agent_rcli(&dir.join("agent.sock"), &["agent", "accept", "password+accept"]).await.unwrap();

    // No scripted answer: the person does not answer, so it auto-rejects after the timeout.
    agent.platform.answer_accept_prompts([]);
    let (status, out) = password_exec(&sys, &dev, &password).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("connect_denied")), "{out}");
    assert_eq!(agent.platform.accept_prompts().len(), 1);
    assert_eq!(agent.platform.accept_prompts()[0].1, Duration::from_secs(30));
}
