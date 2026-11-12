use rcli_protocol::agent::AgentUpdate;
use rcli_testkit::{eventually, StatusCode, TestSystem};

#[tokio::test]
async fn device_reports_its_agent_version() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| !d["agent_version"].is_null()).await;
    assert_eq!(d["agent_version"], env!("CARGO_PKG_VERSION"));
    assert!(d.get("update").is_none(), "{d}");
}

/// A build needs only the file, a semver version and a target: the Server computes its sha256 and size.
#[tokio::test]
async fn an_unsigned_build_uploads_and_bad_versions_and_duplicates_are_refused() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.put_bytes("/api/v1/agent-builds/1.0.0/linux-x86_64", b"bin".to_vec()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // printf bin | sha256sum
    assert_eq!(body["sha256"], "51a1f05af85e342e3c849b47d387086476282d5f50dc240c19216d6edfb1eb5a", "{body}");
    assert_eq!(body["size"], 3, "{body}");

    let (status, body) = sys.api.put_bytes("/api/v1/agent-builds/latest/linux-x86_64", b"bin".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("semver"), "{body}");

    let (status, body) = sys.api.put_bytes("/api/v1/agent-builds/1.0.0/linux-x86_64", b"other".to_vec()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("delete the old one first"), "{body}");
}

/// Agents download builds from the API Address, which a test Server has not learned.
async fn serve_builds_to_agents(sys: &TestSystem) {
    sys.rcli(&["api-address", "set", &format!("http://{}", sys.server.http_addr)]).await.unwrap();
}

/// What an old (signing) Server sends. Its signature field is reserved now, so it decodes and is skipped (see
/// `rcli_protocol::update`'s test); what is left is installed with no key.
fn signed_update(version: &str, build: &[u8]) -> AgentUpdate {
    AgentUpdate {
        version: version.into(),
        target: rcli_agent::target(),
        size: build.len() as u64,
        sha256: rcli_protocol::update::sha256_hex(build),
    }
}

/// During the transition an old Server still sends signed updates: the Agent installs the build, ignoring the signature.
#[tokio::test]
async fn an_agent_installs_an_update_with_no_key_pinned() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| d["agent_version"] == env!("CARGO_PKG_VERSION")).await;
    let build = b"new agent binary";
    sys.upload_build("99.0.0", &rcli_agent::target(), build).await;

    sys.send_agent_update(&d, signed_update("99.0.0", build));

    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "99.0.0").await;
    assert_eq!(agent.platform.installed_updates(), ["99.0.0"]);
}

/// The Agent downloads and installs off its session loop: while an update is under way (held at the install step,
/// after a build larger than a session frame was downloaded), a command still round-trips and the Device stays Online.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_stays_responsive_while_a_large_build_downloads() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| d["agent_version"] == env!("CARGO_PKG_VERSION")).await;
    let build = vec![7u8; 16 << 20];
    sys.upload_build("99.0.0", &rcli_agent::target(), &build).await;
    agent.platform.update(|s| s.hold_install = true);

    sys.send_agent_update(&d, signed_update("99.0.0", &build));
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
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == "99.0.0").await;
}
