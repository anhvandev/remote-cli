//! Admin Device actions: change the Device ID, rename, and set a new Device Password from the Dashboard
//! (`PATCH /api/v1/devices/{device}`). A new password is pushed to the Device's Secret Store live; Admins never
//! read a password back. Operators cannot reach these actions.

use rcli_testkit::{eventually, StatusCode, TestSystem};
use serde_json::json;

/// A short temp directory: the IPC socket path must fit in `SUN_LEN` (104 bytes on macOS).
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli11-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An Online Device whose Agent has an IPC socket trusted as root (the harness is never root), so the test can read
/// the Device's Secret Store with `rcli agent password show`.
async fn device(sys: &TestSystem, name: &str) -> (rcli_testkit::TestAgent, std::path::PathBuf) {
    let dir = temp_dir(name);
    let agent = sys.start_agent_trusting_socket("admin-lab", &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    (agent, dir.join("agent.sock"))
}

#[tokio::test]
async fn an_admin_sets_a_password_the_device_picks_up_live() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "password").await;
    let id = agent.device_id().to_owned();
    let old = agent.device_password().unwrap();
    let new = "correct-horse-battery-staple";

    let (status, body) = sys.api.patch_json(&format!("/api/v1/devices/{id}"), &json!({ "device_password": new })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.to_string().contains(new), "the reply never carries a password: {body}");
    assert_eq!(body["device_id"], id);

    // The Device keeps the new password in its Secret Store, live.
    let shown = sys.agent_rcli(&socket, &["agent", "password", "show"]).await.unwrap();
    assert_eq!(shown.trim(), new);

    // The old password no longer authenticates; the new one does.
    let stale = sys.device_client(&id, &old);
    assert_eq!(stale.get(&format!("/api/v1/devices/{id}")).await.0, StatusCode::UNAUTHORIZED);
    let fresh = sys.device_client(&id, new);
    assert_eq!(fresh.get(&format!("/api/v1/devices/{id}")).await.0, StatusCode::OK);

    // Never logged.
    let (_, audit) = sys.api.get("/api/v1/audit").await;
    assert!(!audit.to_string().contains(new), "the audit log never carries a password: {audit}");

    // A short password is refused and the Device keeps the one it has.
    let (status, body) = sys.api.patch_json(&format!("/api/v1/devices/{id}"), &json!({ "device_password": "short" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(sys.agent_rcli(&socket, &["agent", "password", "show"]).await.unwrap().trim(), new);
}

#[tokio::test]
async fn an_admin_changes_the_device_id_uniquely_and_live() {
    let sys = TestSystem::start().await;
    let (agent, socket) = device(&sys, "id").await;
    let old = agent.device_id().to_owned();
    let other = sys.start_agent("other-pc").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;

    // Duplicate and malformed IDs are refused with a clear error and change nothing.
    let path = format!("/api/v1/devices/{old}");
    let (status, body) = sys.api.patch_json(&path, &json!({ "device_id": other.device_id() })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("already used"), "{body}");
    let (status, body) = sys.api.patch_json(&path, &json!({ "device_id": "12345" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("9 digits"), "{body}");

    let new = "987654321";
    let (status, body) = sys.api.patch_json(&path, &json!({ "device_id": new })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["device_id"], new);

    // Live: `rcli agent info` shows it and the fleet lists the Device under the new ID.
    let info: serde_json::Value =
        serde_json::from_str(&sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.unwrap()).unwrap();
    assert_eq!(info["device_id"], new);
    sys.wait_device(new, |d| d["status"] == "Online").await;
    let (status, body) = sys.api.get(&format!("/api/v1/devices/{old}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // A Controller connects by the new ID.
    assert_eq!(
        sys.device_client(new, &agent.device_password().unwrap()).get(&format!("/api/v1/devices/{new}")).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_admin_renames_a_device() {
    let sys = TestSystem::start().await;
    let (agent, _socket) = device(&sys, "name").await;
    let id = agent.device_id().to_owned();

    let (status, body) = sys.api.patch_json(&format!("/api/v1/devices/{id}"), &json!({ "name": "renamed-pc" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "renamed-pc");
    assert_eq!(sys.api.get("/api/v1/devices/renamed-pc").await.0, StatusCode::OK);
    let (status, body) = sys.api.patch_json(&format!("/api/v1/devices/{id}"), &json!({ "name": "" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn operators_cannot_change_a_device_identity() {
    let sys = TestSystem::start().await;
    let (agent, _socket) = device(&sys, "operator").await;
    let id = agent.device_id().to_owned();
    let operator = sys.api.with_token(Some(&sys.create_user("op-11", "operator").await));

    for body in [json!({ "device_password": "longenough" }), json!({ "device_id": "987654321" }), json!({ "name": "x" })] {
        let (status, body) = operator.patch_json(&format!("/api/v1/devices/{id}"), &body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "admin_required", "{body}");
    }
}

#[tokio::test]
async fn an_offline_device_can_be_renamed_but_its_id_and_password_need_it_online() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("gone-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let id = agent.device_id().to_owned();
    agent.stop();
    sys.advance(31);
    sys.wait_device(&id, |d| d["status"] == "Offline").await;

    // The ID and password are pushed to the Device, so an Offline Device is refused with a clear error and nothing
    // changes (no queueing).
    let path = format!("/api/v1/devices/{id}");
    let (status, body) = sys.api.patch_json(&path, &json!({ "device_password": "longenough" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "device_not_connected", "{body}");
    let (status, body) = sys.api.patch_json(&path, &json!({ "device_id": "987654321" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(sys.api.get(&path).await.1["device_id"], id);

    // A rename is Server-side only and works while the Device is Offline.
    let (status, body) = sys.api.patch_json(&path, &json!({ "name": "gone-pc-2" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "gone-pc-2");

    // The Agent was never told (it was Offline) and cannot be reached later without a reconnect: that is the error.
    eventually(|| async { (sys.api.get(&path).await.0 == StatusCode::OK).then_some(()) }).await;
}
