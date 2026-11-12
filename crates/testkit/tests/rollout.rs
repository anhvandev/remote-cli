use rcli_testkit::{eventually, FakePlatform, StatusCode, TestAgent, TestSystem};
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
    serve_builds_to_agents(&sys).await;
    let a = sys.start_agent("pc-a").await;
    let b = sys.start_agent("pc-b").await;
    for agent in [&a, &b] {
        sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    }
    let build = temp_file("rcli", b"new agent binary");
    let key = temp_file("update.key", &sys.update_signing_key);
    let target = rcli_agent::target();
    sys.rcli(&["rollout", "publish", &build, "--version", "0.2.0", "--target", &target, "--key", &key]).await.unwrap();

    sys.rcli(&["rollout", "0.2.0", "--device", a.device_id()]).await.unwrap();
    let d = sys.wait_device(a.device_id(), |d| d["update"]["status"] == "updated").await;
    assert_eq!(d["agent_version"], "0.2.0");
    assert_eq!(a.platform.installed_updates(), ["0.2.0"]);
    let list = sys.rcli(&["devices"]).await.unwrap();
    let row = list.lines().find(|l| l.contains("pc-b")).unwrap();
    assert!(row.contains("0.1.0") && !row.contains("0.2.0"), "{list}");

    sys.rcli(&["rollout", "0.2.0", "--all"]).await.unwrap();
    sys.wait_device(b.device_id(), |d| d["agent_version"] == "0.2.0" && d["update"]["status"] == "updated").await;
    let list = sys.rcli(&["devices"]).await.unwrap();
    assert!(list.lines().find(|l| l.contains("pc-b")).unwrap().contains("updated"), "{list}");
}

/// An Agent pinned to another update key refuses a build signed with the Server's key, and reports why.
#[tokio::test]
async fn a_build_not_signed_with_the_agents_pinned_key_is_rejected_and_reported() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    let platform = FakePlatform::new("pc-1");
    let mut creds = sys.register(&platform).await.unwrap();
    creds.update_key = rcli_protocol::update::generate_key().unwrap().1;
    let mut agent = TestAgent::new(sys.server_access(), platform, creds);
    agent.start();
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &temp_file("rcli", b"new agent binary"), "6.6.6", &key).await.unwrap();

    sys.rcli(&["rollout", "6.6.6", "--all"]).await.unwrap();
    let d = sys.wait_device(agent.device_id(), |d| d["update"]["status"] == "failed").await;
    assert!(d["update"]["error"].as_str().unwrap().contains("bad signature"), "{d}");
    assert_eq!(d["agent_version"], "0.1.0");
    assert!(agent.platform.installed_updates().is_empty());
}

/// With no API Address the Agent cannot download a rolled-out build; it reports that instead of retrying forever.
#[tokio::test]
async fn an_agent_with_no_api_address_reports_the_update_failed() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("pc-1").await;
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &temp_file("rcli", b"new agent binary"), "0.2.0", &key).await.unwrap();

    sys.rcli(&["rollout", "0.2.0", "--all"]).await.unwrap();
    let d = sys.wait_device(agent.device_id(), |d| d["update"]["status"] == "failed").await;
    assert!(d["update"]["error"].as_str().unwrap().contains("API Address"), "{d}");
}

/// Agents download rolled-out builds from the API Address, which a test Server has not learned.
async fn serve_builds_to_agents(sys: &TestSystem) {
    sys.rcli(&["api-address", "set", &format!("http://{}", sys.server.http_addr)]).await.unwrap();
}

/// The Agent downloads and installs off its session loop: while an update is under way (held at the install step,
/// after a build larger than a session frame was downloaded), a command still round-trips and the Device stays Online.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_stays_responsive_while_a_large_build_downloads() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    let agent = sys.start_agent("pc-1").await;
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.1.0").await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &temp_file("rcli", &vec![7u8; 16 << 20]), "0.2.0", &key).await.unwrap();
    agent.platform.update(|s| s.hold_install = true);

    sys.rcli(&["rollout", "0.2.0", "--all"]).await.unwrap();
    eventually(|| async {
        let mut installing = false;
        agent.platform.update(|s| installing = s.installing);
        installing.then_some(())
    })
    .await;
    let exec = ["exec", agent.device_id(), "--", "echo", "hi"];
    let out = tokio::time::timeout(std::time::Duration::from_secs(5), sys.rcli(&exec)).await.expect("the command answers during the update");
    assert!(out.unwrap().contains("hi"));
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    agent.platform.update(|s| s.hold_install = false);
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "0.2.0" && d["update"]["status"] == "updated").await;
}

/// `rcli rollout publish` of `build` as `version` for this machine's target, signed with `key`.
async fn publish(sys: &TestSystem, build: &str, version: &str, key: &str) -> anyhow::Result<String> {
    let target = rcli_agent::target();
    sys.rcli(&["rollout", "publish", build, "--version", version, "--target", &target, "--key", key]).await
}

/// The Server checks the signature at upload: a wrong one is refused and leaves nothing behind, so the same
/// version + target can then be published with the right signature; publishing it twice still fails.
#[tokio::test]
async fn a_wrong_signature_is_refused_at_upload_and_the_retry_succeeds() {
    let sys = TestSystem::start().await;
    let rogue_key = temp_path("rogue.key");
    sys.rcli(&["rollout", "keygen", &rogue_key]).await.unwrap();
    let build = temp_file("rcli", b"agent binary");
    let key = temp_file("update.key", &sys.update_signing_key);
    let target = rcli_agent::target();

    let err = publish(&sys, &build, "1.0.0", &rogue_key).await.unwrap_err();
    assert!(err.to_string().contains("signature does not match this version, target and file"), "{err}");
    // The right key over other bytes is wrong too.
    let path = format!("/api/v1/agent-builds/1.0.0/{target}?signature={}", sys.sign_build("1.0.0", &target, b"other"));
    let (status, body) = sys.api.put_bytes(&path, b"agent binary".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "bad_signature", "{body}");

    let out = publish(&sys, &build, "1.0.0", &key).await.unwrap();
    assert_eq!(out, format!("published 1.0.0 for {target}\n"));
    let err = publish(&sys, &build, "1.0.0", &key).await.unwrap_err();
    assert!(err.to_string().contains("exists"), "{err}");
}

#[tokio::test]
async fn uploads_are_refused_while_no_update_key_is_configured() {
    let mut sys = TestSystem::start().await;
    sys.config.update_public_key = None;
    sys.restart(Some(TestSystem::master_key())).await;
    let key = temp_file("update.key", &sys.update_signing_key);

    let err = publish(&sys, &temp_file("rcli", b"agent binary"), "1.0.0", &key).await.unwrap_err();
    assert!(err.to_string().contains("no update public key"), "{err}");
}

#[tokio::test]
async fn unsigned_builds_and_unknown_versions_are_refused() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.put_bytes("/api/v1/agent-builds/1.0.0/linux-x86_64", b"bin".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("unsigned"), "{body}");

    let (status, body) = sys.api.post_json("/api/v1/agent-builds/1.0.0/rollout", &json!({"all": true})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
