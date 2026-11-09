use rcli_testkit::{StatusCode, TestSystem};
use serde_json::json;

#[tokio::test]
async fn device_reports_its_agent_version() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| !d["agent_version"].is_null()).await;
    assert_eq!(d["agent_version"], "0.1.0");
    assert!(d["update"].is_null(), "{d}");
}

/// A path in a fresh temp directory.
fn temp_path(name: &str) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("rcli-update-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_str().unwrap().to_owned()
}

fn temp_file(name: &str, bytes: &[u8]) -> String {
    let path = temp_path(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[tokio::test]
async fn signed_build_rolls_out_to_selected_devices_then_to_all() {
    let sys = TestSystem::start().await;
    let a = sys.start_agent("pc-a").await;
    let b = sys.start_agent("pc-b").await;
    for agent in [&a, &b] {
        sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    }
    let build = temp_file("rcli", b"new agent binary");
    let key = temp_file("update.key", &sys.update_signing_key);
    let target = rcli_agent::target();
    sys.rcli(&["update", "publish", &build, "--version", "0.2.0", "--target", &target, "--key", &key]).await.unwrap();

    sys.rcli(&["update", "rollout", "0.2.0", "--device", a.device_id()]).await.unwrap();
    let d = sys.wait_device(a.device_id(), |d| d["update"]["status"] == "updated").await;
    assert_eq!(d["agent_version"], "0.2.0");
    assert_eq!(a.platform.installed_updates(), ["0.2.0"]);
    let list = sys.rcli(&["devices"]).await.unwrap();
    let row = list.lines().find(|l| l.contains("pc-b")).unwrap();
    assert!(row.contains("0.1.0") && !row.contains("0.2.0"), "{list}");

    sys.rcli(&["update", "rollout", "0.2.0", "--all"]).await.unwrap();
    sys.wait_device(b.device_id(), |d| d["agent_version"] == "0.2.0" && d["update"]["status"] == "updated").await;
    let list = sys.rcli(&["devices"]).await.unwrap();
    assert!(list.lines().find(|l| l.contains("pc-b")).unwrap().contains("updated"), "{list}");
}

#[tokio::test]
async fn build_signed_with_another_key_is_rejected_and_reported() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("pc-1").await;
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    let rogue_key = temp_path("rogue.key");
    let out = sys.rcli(&["update", "keygen", &rogue_key]).await.unwrap();
    assert!(out.starts_with("public key "), "{out}");
    let build = temp_file("rcli", b"malicious binary");
    let target = rcli_agent::target();
    sys.rcli(&["update", "publish", &build, "--version", "6.6.6", "--target", &target, "--key", &rogue_key])
        .await
        .unwrap();

    sys.rcli(&["update", "rollout", "6.6.6", "--all"]).await.unwrap();
    let d = sys.wait_device(agent.device_id(), |d| d["update"]["status"] == "failed").await;
    assert!(d["update"]["error"].as_str().unwrap().contains("bad signature"), "{d}");
    assert_eq!(d["agent_version"], "0.1.0");
    assert!(agent.platform.installed_updates().is_empty());
}

#[tokio::test]
async fn unsigned_builds_and_unknown_versions_are_refused() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.put_bytes("/api/v1/agent-builds/1.0.0/linux-x86_64", b"bin".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("unsigned"), "{body}");

    let (status, body) = sys.api.post_json("/api/v1/agent-builds/1.0.0/rollout", &json!({"all": true})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let op = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, _) = op.post_json("/api/v1/agent-builds/1.0.0/rollout", &json!({"all": true})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
