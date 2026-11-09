//! Ticket 09: the unlock password lives on the Device, and `unlock` types it there.

use rcli_testkit::{eventually, FakePlatform, Input, SessionState, StatusCode, TestSystem};
use serde_json::json;

const PASSWORD: &str = "hunter2-very-secret";

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli09-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn unlock_types_the_password_stored_on_the_device_without_ever_revealing_it() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("unlock");
    let platform = FakePlatform::new("lab-pc");
    platform.set_session_state(SessionState::Locked);
    let agent = sys.start_agent_trusting_socket_on(platform, &dir).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    let alice_token = sys.create_user("alice", "operator").await;
    let alice = sys.api.with_token(Some(&alice_token));
    let mut seen = Vec::new(); // every Controller-visible response

    // Set at the Device, over root-only local IPC.
    let out = sys.agent_rcli(&socket, &["agent", "unlock-password", "set", "--password", PASSWORD]).await.unwrap();
    seen.push(out);

    // The Server has no unlock-password endpoint any more.
    let (status, body) = sys.api.get(&format!("/api/v1/devices/{dev}/unlock-password")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Needs the Control Lock.
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "control_lock_required");
    assert!(agent.platform.inputs().is_empty());

    sys.rcli_as(&alice_token, &["lock", "acquire", "lab-pc"]).await.unwrap();
    let out = sys.rcli_as(&alice_token, &["unlock", "lab-pc", "--json"]).await.unwrap();
    seen.push(out);
    assert_eq!(agent.platform.inputs(), vec![Input::Text(PASSWORD.into()), Input::Key("enter".into())]);
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["images"].is_array());

    let (_, entries) = sys.api.get("/api/v1/audit").await;
    let unlocks: Vec<_> = entries.as_array().unwrap().iter().filter(|e| e["action"] == "unlock").collect();
    assert_eq!(unlocks.len(), 2, "{entries}");
    assert_eq!(unlocks[0]["user"], "alice");
    assert_eq!(unlocks[0]["params"], json!({}), "the password is not in the audit entry");
    assert_eq!(unlocks[0]["result"]["error"], serde_json::Value::Null);
    seen.push(entries.to_string());
    seen.push(sys.rcli(&["audit"]).await.unwrap());
    seen.push(sys.api.get(&format!("/api/v1/devices/{dev}")).await.1.to_string());
    seen.push(sys.api.get("/api/v1/devices").await.1.to_string());
    for s in &seen {
        assert!(!s.contains(PASSWORD), "password leaked: {s}");
    }

    // Cleared: unlock fails clearly and the Agent types nothing.
    sys.agent_rcli(&socket, &["agent", "unlock-password", "clear"]).await.unwrap();
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "unlock_password_not_set");
    let err = sys.rcli_as(&alice_token, &["unlock", "lab-pc"]).await.unwrap_err();
    assert!(err.to_string().contains("unlock_password_not_set"), "{err}");
    assert_eq!(agent.platform.inputs().len(), 4, "only the two unlocks above typed anything");
}

#[tokio::test]
async fn each_device_types_its_own_password() {
    let sys = TestSystem::start().await;
    let (a, a_dir) = online(&sys, "a", FakePlatform::new("a")).await;
    let (b, b_dir) = online(&sys, "b", FakePlatform::new("b")).await;
    // Replacing keeps only the newest password.
    sys.agent_rcli(&a_dir.join("agent.sock"), &["agent", "unlock-password", "set", "--password", "pw-a"]).await.unwrap();
    sys.agent_rcli(&a_dir.join("agent.sock"), &["agent", "unlock-password", "set", "--password", "pw-a2"]).await.unwrap();
    sys.agent_rcli(&b_dir.join("agent.sock"), &["agent", "unlock-password", "set", "--password", "pw-b"]).await.unwrap();

    sys.rcli(&["lock", "acquire", "b"]).await.unwrap();
    sys.rcli(&["unlock", "b", "--json"]).await.unwrap();
    assert_eq!(b.platform.inputs()[0], Input::Text("pw-b".into()));
    sys.rcli(&["lock", "acquire", "a"]).await.unwrap();
    sys.rcli(&["unlock", "a", "--json"]).await.unwrap();
    assert_eq!(a.platform.inputs()[0], Input::Text("pw-a2".into()));
}

/// Starts a Device with a root-trusting IPC socket so the test can set its unlock password.
async fn online(sys: &TestSystem, name: &str, platform: FakePlatform) -> (rcli_testkit::TestAgent, std::path::PathBuf) {
    let dir = temp_dir(name);
    let agent = sys.start_agent_trusting_socket_on(platform, &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    eventually(|| async { dir.join("agent.sock").exists().then_some(()) }).await;
    (agent, dir)
}

#[tokio::test]
async fn only_root_may_set_or_clear_the_unlock_password() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("gate");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    for command in [
        vec!["agent", "unlock-password", "set", "--password", PASSWORD],
        vec!["agent", "unlock-password", "clear"],
    ] {
        let out = sys.agent_rcli(&socket, &command).await;
        if rcli_testkit::running_as_root() {
            out.unwrap();
        } else {
            assert!(out.unwrap_err().to_string().contains("root"), "a non-root caller must be refused");
        }
    }
    let _ = agent;
}
