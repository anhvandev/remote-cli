//! Changing the Device Password, Device ID, name and Server from the Device (`rcli agent ...` over local IPC).
//!
//! The test harness runs unprivileged, so `start_agent_with_socket` serves a socket that trusts every caller as
//! root. The permission gate itself (peer credentials, every change refused) is covered by
//! `ipc::tests::only_root_may_show_or_change_the_device` and ticket 03's `password_show_is_refused_to_non_root`.

use rcli_testkit::{eventually, StatusCode, TestSystem};

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli06-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A Device with a running Agent and an IPC socket; its directory also holds the Secret Store.
async fn device(sys: &TestSystem, name: &str) -> (rcli_testkit::TestAgent, std::path::PathBuf) {
    let dir = temp_dir(name);
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    (agent, dir.join("agent.sock"))
}

#[tokio::test]
async fn a_password_change_needs_eight_characters_and_keeps_open_sessions() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "password").await;
    let id = agent.device_id().to_owned();
    let old = agent.device_password().unwrap();

    let err = sys.agent_rcli(&socket, &["agent", "password", "set", "--password", "short"]).await.unwrap_err();
    assert!(err.to_string().contains("8 characters"), "{err}");

    // An open session (the old password) survives the change, stream included.
    let open = sys.device_client(&id, &old);
    let streaming = open.watch(&format!("/api/v1/devices/{id}/lock/watch")).await;

    let new = "correct-horse-battery";
    sys.agent_rcli(&socket, &["agent", "password", "set", "--password", new]).await.unwrap();
    // The already-open stream keeps running (it authenticated when it opened).
    let mut streaming = streaming;
    assert!(streaming.chunk().await.unwrap().is_some(), "an open session keeps working");
    assert_eq!(open.get(&format!("/api/v1/devices/{id}")).await.0, StatusCode::UNAUTHORIZED, "the old password no longer authenticates");

    // A new connection needs the new password.
    let stale = sys.device_client(&id, &old);
    assert_eq!(stale.get(&format!("/api/v1/devices/{id}")).await.0, StatusCode::UNAUTHORIZED);
    let fresh = sys.device_client(&id, new);
    assert_eq!(fresh.get(&format!("/api/v1/devices/{id}")).await.0, StatusCode::OK);

    // The Device keeps the new password in its Secret Store.
    let shown = sys.agent_rcli(&socket, &["agent", "password", "show"]).await.unwrap();
    assert_eq!(shown.trim(), new);
    drop(streaming);
}

#[tokio::test]
async fn a_device_id_change_is_unique_and_the_old_id_is_not_found() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "id").await;
    let other = sys.start_agent("other-pc").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;

    let err = sys.agent_rcli(&socket, &["agent", "id", "set", other.device_id()]).await.unwrap_err();
    assert!(err.to_string().contains("already used"), "{err}");
    let err = sys.agent_rcli(&socket, &["agent", "id", "set", "12345"]).await.unwrap_err();
    assert!(err.to_string().contains("9 digits"), "{err}");

    let old = agent.device_id().to_owned();
    let new = "987654321";
    sys.agent_rcli(&socket, &["agent", "id", "set", new]).await.unwrap();

    sys.wait_device(new, |d| d["status"] == "Online").await;
    let info: serde_json::Value =
        serde_json::from_str(&sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.unwrap()).unwrap();
    assert_eq!(info["device_id"], new);

    // A Controller still using the old ID gets a clear "not found".
    let stale = sys.device_client(&old, &agent.device_password().unwrap());
    let (status, body) = stale.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = sys.api.get(&format!("/api/v1/devices/{old}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "device_not_found");

    // New connections use the new ID.
    let fresh = sys.device_client(new, &agent.device_password().unwrap());
    assert_eq!(fresh.get(&format!("/api/v1/devices/{new}")).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_name_change_applies_at_once() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "name").await;

    sys.agent_rcli(&socket, &["agent", "name", "set", "renamed-pc"]).await.unwrap();
    sys.wait_device(agent.device_id(), |d| d["name"] == "renamed-pc").await;

    // The new name resolves the Device.
    let (status, _) = sys.api.get("/api/v1/devices/renamed-pc").await;
    assert_eq!(status, StatusCode::OK);
    let err = sys.agent_rcli(&socket, &["agent", "name", "set", ""]).await.unwrap_err();
    assert!(err.to_string().contains("1-255"), "{err}");
}

#[tokio::test]
async fn a_server_address_change_reconnects_without_a_restart() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "server").await;
    let id = agent.device_id().to_owned();
    let key = sys.server_key();
    let real = agent.status.endpoint().addr;

    // A malformed Server Key is refused and changes nothing.
    let bad = sys.agent_rcli(&socket, &["agent", "server", "set", "localhost:1", "--key", "nonsense"]).await;
    assert!(bad.is_err(), "a malformed Server Key is refused");

    // Point it at a dead address: the same Agent process drops the session and tries the new Server.
    sys.agent_rcli(&socket, &["agent", "server", "set", "localhost:1", "--key", &key]).await.unwrap();
    let down: serde_json::Value = eventually(|| async {
        let out = sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.ok()?;
        let info: serde_json::Value = serde_json::from_str(&out).ok()?;
        (info["connected"] == false && info["server"] == "localhost:1").then_some(info)
    })
    .await;
    assert_eq!(down["server"], "localhost:1");

    // Point it back: no restart, and the Server sees this same Device Online again.
    sys.agent_rcli(&socket, &["agent", "server", "set", &real, "--key", &key]).await.unwrap();
    sys.wait_device(&id, |d| d["status"] == "Online").await;
    let info: serde_json::Value = eventually(|| async {
        let out = sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.ok()?;
        let info: serde_json::Value = serde_json::from_str(&out).ok()?;
        (info["connected"] == true).then_some(info)
    })
    .await;
    assert_eq!(info["server"], real);
}
