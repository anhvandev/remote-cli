//! `rcli upgrade`: fetch the build rolled out for this OS/arch, verify it with the pinned update key, replace the
//! binary. The replacement path is a temp file here (the seam), never the running test binary.

use rcli_testkit::{temp_dir, TestSystem};

fn temp_file(name: &str, bytes: &[u8]) -> String {
    let path = temp_dir("builds").join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_owned()
}

/// Publishes `build` as `version` for this machine's target, signed with `key`.
async fn publish(sys: &TestSystem, build: &str, version: &str, key: &str) {
    let target = rcli_agent::target();
    sys.rcli(&["rollout", "publish", build, "--version", version, "--target", &target, "--key", key]).await.unwrap();
}

fn http_addr(sys: &TestSystem) -> String {
    format!("http://{}", sys.server.http_addr)
}

#[tokio::test]
async fn a_controller_only_machine_upgrades_itself() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("controller");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    let build = temp_file("rcli", b"new rcli binary");
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &build, "9.9.9", &key).await;

    let target = temp_dir("controller-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();
    let out = sys
        .rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()])
        .await
        .unwrap();

    assert!(out.contains("9.9.9"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"new rcli binary");
}

/// An installed Device upgrades to the build rolled out to it, not the newest publish (the rollout is what the
/// Server hands out).
#[tokio::test]
async fn an_installed_device_upgrades_to_the_build_rolled_out_to_it() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("device");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &temp_file("rolled", b"rolled-out build"), "1.0.0", &key).await;
    sys.advance(60);
    publish(&sys, &temp_file("newest", b"newest build"), "2.0.0", &key).await;

    let agent_dir = temp_dir("device-agent");
    let agent = sys.start_agent_with_socket("lab-pc", &agent_dir).await;
    sys.rcli(&["rollout", "1.0.0", "--device", agent.device_id()]).await.unwrap();

    let target = temp_dir("device-target").join("rcli");
    std::fs::write(&target, b"installed build").unwrap();
    let socket = agent_dir.join("agent.sock").to_str().unwrap().to_owned();
    let out = sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap(), "--socket", &socket]).await.unwrap();

    assert!(out.contains("1.0.0"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"rolled-out build");
}

#[tokio::test]
async fn a_bad_signature_is_rejected_and_the_old_binary_is_kept() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("bad-signature");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    let rogue = temp_dir("rogue").join("rogue.key");
    sys.rcli(&["rollout", "keygen", rogue.to_str().unwrap()]).await.unwrap();
    let build = temp_file("rcli", b"malicious rcli binary");
    publish(&sys, &build, "6.6.6", rogue.to_str().unwrap()).await;

    let target = temp_dir("bad-signature-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();
    let err = sys
        .rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()])
        .await
        .unwrap_err();

    assert!(err.to_string().contains("bad signature"), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old rcli binary");
}

#[tokio::test]
async fn upgrade_fetches_the_newest_published_build_for_this_target() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("newest");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    let key = temp_file("update.key", &sys.update_signing_key);
    publish(&sys, &temp_file("old", b"old build"), "2.0.0", &key).await;
    sys.advance(60);
    publish(&sys, &temp_file("new", b"new build"), "1.0.0", &key).await;

    let target = temp_dir("newest-target").join("rcli");
    std::fs::write(&target, b"installed build").unwrap();
    let out = sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert!(out.contains("1.0.0"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
}

/// An on-path attacker can serve the real CA (so the fingerprint matches) but cannot sign another update key
/// without the Server Key's secret: `rcli setup` refuses the pin, so `rcli upgrade` can never trust it.
#[tokio::test]
async fn a_server_that_cannot_prove_the_update_key_is_refused() {
    use axum::{routing::get, Router};
    let sys = TestSystem::start().await;
    let ca = sys.server.ca_pem.clone();
    let impostor = Router::new().route("/api/v1/server-ca", get(move || async move { ca.clone() })).route(
        "/api/v1/update-key",
        get(|| async { axum::Json(serde_json::json!({ "update_key": "00".repeat(32), "mac": "00".repeat(32) })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, impostor).await });
    let dir = temp_dir("impostor");

    let err = sys
        .rcli_in(&dir, &["setup", "--server", &format!("http://{addr}"), "--key", &sys.server_key()])
        .await
        .unwrap_err();

    assert!(err.to_string().contains("did not prove the update key"), "{err}");
    assert!(!dir.join("config.json").exists(), "nothing was pinned");
}

#[tokio::test]
async fn no_build_for_this_os_and_arch_is_a_clear_error() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("no-build");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    let target = temp_dir("no-build-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();

    let err = sys
        .rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()])
        .await
        .unwrap_err();

    assert!(err.to_string().contains(rcli_agent::target().as_str()), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old rcli binary");
}
