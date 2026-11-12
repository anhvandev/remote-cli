use rcli_protocol::agent::{server_message, AgentUpdate, ApiAddress};
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

async fn set_minimum_version(sys: &TestSystem, version: &str) {
    let (status, body) = sys.api.put_json("/api/v1/server-settings/minimum-version", &serde_json::json!({ "version": version })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Raising the Minimum Version reaches an Agent already connected: it installs the Latest Build and reconnects with it.
#[tokio::test]
async fn an_agent_below_the_minimum_version_updates_itself_to_the_latest_build() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| d["agent_version"] == env!("CARGO_PKG_VERSION")).await;
    assert_eq!(d["needs_update"], false, "{d}");
    assert!(d["update_error"].is_null(), "{d}");
    sys.upload_build("99.0.0", &rcli_agent::target(), b"new agent binary").await;

    set_minimum_version(&sys, "1.0.0").await;

    let d = sys.wait_device(agent.device_id(), |d| d["agent_version"] == "99.0.0").await;
    assert_eq!(d["needs_update"], false, "{d}");
    assert_eq!(agent.platform.installed_updates(), ["99.0.0"]);
}

/// Below the Minimum Version but the Latest Build is not newer, or at the Minimum Version: no update is sent, even
/// an hour later. The Device still shows it needs one, in the API and in `rcli devices`.
#[tokio::test]
async fn no_agent_update_without_a_newer_build_or_at_the_minimum_version() {
    let sys = TestSystem::start().await;
    serve_builds_to_agents(&sys).await;
    sys.upload_build("0.0.1", &rcli_agent::target(), b"old agent binary").await;
    set_minimum_version(&sys, "1.0.0").await;
    let agent = sys.start_agent("pc-1").await;
    sys.wait_device(agent.device_id(), |d| d["agent_version"] == env!("CARGO_PKG_VERSION") && d["needs_update"] == true).await;
    let out = sys.rcli(&["devices"]).await.unwrap();
    assert!(out.contains("update needed"), "{out}");

    set_minimum_version(&sys, env!("CARGO_PKG_VERSION")).await;
    sys.upload_build("99.0.0", &rcli_agent::target(), b"new agent binary").await;
    sys.advance(3600);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    assert!(agent.platform.installed_updates().is_empty());
    let d = sys.wait_device(agent.device_id(), |_| true).await;
    assert_eq!(d["needs_update"], false, "{d}");
}

/// An Agent below the Minimum Version on connecting gets the Latest Build; a failure (here: no API Address to download
/// from) shows its reason until the Minimum Version is lowered to the Device's version, comes back when raised again,
/// and is retried after an hour.
#[tokio::test]
async fn a_failed_self_update_shows_its_reason_and_is_retried_after_an_hour() {
    let sys = TestSystem::start().await;
    sys.upload_build("99.0.0", &rcli_agent::target(), b"new agent binary").await;
    set_minimum_version(&sys, "1.0.0").await;

    let agent = sys.start_agent("pc-1").await;
    let d = sys.wait_device(agent.device_id(), |d| !d["update_error"].is_null()).await;
    assert!(d["update_error"].as_str().unwrap().contains("API Address"), "{d}");
    assert_eq!(d["needs_update"], true, "{d}");
    let out = sys.rcli(&["devices"]).await.unwrap();
    assert!(out.contains("API Address"), "{out}");

    set_minimum_version(&sys, env!("CARGO_PKG_VERSION")).await;
    let d = sys.wait_device(agent.device_id(), |_| true).await;
    assert!(d["update_error"].is_null(), "{d}");
    set_minimum_version(&sys, "1.0.0").await;
    let d = sys.wait_device(agent.device_id(), |d| !d["update_error"].is_null()).await;

    // Agents learn the API Address on connecting; hand it to this one without reconnecting, which would retry at once.
    serve_builds_to_agents(&sys).await;
    let url = format!("http://{}", sys.server.http_addr);
    sys.server.state.agents.send(d["id"].as_str().unwrap().parse().unwrap(), server_message::Msg::ApiAddress(ApiAddress { url }));
    sys.advance(3600);
    let d = sys.wait_device(agent.device_id(), |d| d["agent_version"] == "99.0.0").await;
    assert!(d["update_error"].is_null(), "{d}");
    assert_eq!(agent.platform.installed_updates(), ["99.0.0"]);
}
