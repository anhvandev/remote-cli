//! `rcli upgrade`: fetch the build rolled out for this OS/arch, verify it with the pinned update key, replace the
//! binary. The replacement path is a temp file here (the seam), never the running test binary.

use std::path::PathBuf;

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

/// Publishes `build` and rolls it out to every Device, making it the fleet's build.
async fn publish_to_fleet(sys: &TestSystem, build: &str, version: &str, key: &str) {
    publish(sys, build, version, key).await;
    sys.rcli(&["rollout", version, "--all"]).await.unwrap();
}

fn http_addr(sys: &TestSystem) -> String {
    format!("http://{}", sys.server.http_addr)
}

/// `rcli install` pins the update key the Device got at registration, machine-wide, so `rcli upgrade` checks builds
/// against it with no further setup.
#[tokio::test]
async fn an_installed_machine_upgrades_itself_with_the_update_key_pinned_at_install() {
    let sys = TestSystem::start().await;
    sys.rcli(&["api-address", "set", &http_addr(&sys)]).await.unwrap();
    let dir = temp_dir("controller");
    sys.install(&dir).await.unwrap();
    let build = temp_file("rcli", b"new rcli binary");
    let key = temp_file("update.key", &sys.update_signing_key);
    publish_to_fleet(&sys, &build, "9.9.9", &key).await;

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
    sys.controller_config(&dir, None).await;
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
    sys.controller_config(&dir, None).await;
    // This machine pinned another update key than the one the build is signed with.
    let path = rcli::config::machine_path(Some(&dir));
    let mut machine = rcli::config::load_machine(&path).unwrap().unwrap();
    machine.update_key = hex::encode(rcli_protocol::update::generate_key().unwrap().1);
    rcli::config::save_machine(&path, &machine).unwrap();
    let key = temp_file("update.key", &sys.update_signing_key);
    publish_to_fleet(&sys, &temp_file("rcli", b"new rcli binary"), "6.6.6", &key).await;

    let target = temp_dir("bad-signature-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();
    let err = sys
        .rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()])
        .await
        .unwrap_err();

    assert!(err.to_string().contains("bad signature"), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old rcli binary");
}

/// A Controller-only machine gets the fleet's build: the last one rolled out to every Device, not a newer publish
/// or a canary rolled out to one Device.
#[tokio::test]
async fn upgrade_fetches_the_build_rolled_out_to_the_fleet() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("fleet");
    sys.controller_config(&dir, None).await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish_to_fleet(&sys, &temp_file("fleet", b"fleet build"), "1.0.0", &key).await;
    sys.advance(60);
    publish(&sys, &temp_file("canary", b"canary build"), "2.0.0", &key).await;
    let agent = sys.start_agent_with_socket("canary-pc", &temp_dir("fleet-agent")).await;
    sys.rcli(&["rollout", "2.0.0", "--device", agent.device_id()]).await.unwrap();

    let target = temp_dir("fleet-target").join("rcli");
    std::fs::write(&target, b"installed build").unwrap();
    let out = sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert!(out.contains("1.0.0"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"fleet build");
}

#[tokio::test]
async fn no_build_for_this_os_and_arch_is_a_clear_error() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("no-build");
    sys.controller_config(&dir, None).await;
    let target = temp_dir("no-build-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();

    let err = sys
        .rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()])
        .await
        .unwrap_err();

    assert!(err.to_string().contains(rcli_agent::target().as_str()), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"old rcli binary");
}

/// Which build a Device ID is on is not public: the lookup `rcli upgrade` makes needs the Server Key.
#[tokio::test]
async fn the_build_lookup_needs_the_server_key() {
    let sys = TestSystem::start().await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish_to_fleet(&sys, &temp_file("fleet", b"fleet build"), "1.0.0", &key).await;
    let path = format!("/api/v1/agent-builds/{}", rcli_agent::target());

    let (status, _) = sys.api.with_token(None).get_bytes(&path).await;
    assert_eq!(status, rcli_testkit::StatusCode::UNAUTHORIZED);
    let (status, bytes) = reqwest::Client::new()
        .get(format!("{}{path}", http_addr(&sys)))
        .header("x-rcli-server-key", sys.server_key())
        .send()
        .await
        .map(|r| (r.status(), r))
        .unwrap();
    assert_eq!(status, rcli_testkit::StatusCode::OK);
    // sha256("fleet build")
    assert_eq!(bytes.headers()["x-rcli-sha256"], "02c8643101b1e81c151c138aeb814cda1c6cbef317e64fb43d9172f1d9efd1ea");
    assert_eq!(bytes.content_length(), Some(11));
    assert_eq!(bytes.bytes().await.unwrap().as_ref(), b"fleet build");
    // A resumed download asks for the rest.
    let part = reqwest::Client::new()
        .get(format!("{}{path}", http_addr(&sys)))
        .header("x-rcli-server-key", sys.server_key())
        .header("range", "bytes=6-")
        .send()
        .await
        .unwrap();
    assert_eq!(part.status(), rcli_testkit::StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.headers()["content-range"], "bytes 6-10/11");
    assert_eq!(part.bytes().await.unwrap().as_ref(), b"build");
}

/// Publishes `build` to the fleet for a Controller set up in a new `name` dir, and leaves `part` as the partial
/// download `rcli upgrade` finds in its data dir. Returns the dir, the `.part` path and the binary to replace.
async fn with_partial(sys: &TestSystem, name: &str, build: &[u8], part: &[u8]) -> (PathBuf, PathBuf, PathBuf) {
    let dir = temp_dir(name);
    sys.controller_config(&dir, None).await;
    let key = temp_file("update.key", &sys.update_signing_key);
    publish_to_fleet(sys, &temp_file(&format!("{name}-build"), build), "3.0.0", &key).await;
    let updates = dir.join("updates");
    std::fs::create_dir_all(&updates).unwrap();
    // The routine names the file after the build's sha256, so a resume never mixes two builds.
    let part_path = updates.join(format!("{}.part", rcli_protocol::update::sha256_hex(build)));
    std::fs::write(&part_path, part).unwrap();
    let target = temp_dir(&format!("{name}-target")).join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();
    (dir, part_path, target)
}

/// An interrupted download resumes from the bytes already on disk.
#[tokio::test]
async fn upgrade_completes_from_a_partial_download() {
    let sys = TestSystem::start().await;
    let (dir, part, target) = with_partial(&sys, "resume", b"new rcli binary", b"new rcli").await;

    let out = sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert!(out.contains("3.0.0"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"new rcli binary");
    assert!(!part.exists(), "the download is left behind");
}

/// A partial download holding the wrong bytes fails the sha256 check, is thrown away and fetched again from zero.
#[tokio::test]
async fn a_partial_download_with_wrong_bytes_is_discarded() {
    let sys = TestSystem::start().await;
    let (dir, part, target) = with_partial(&sys, "corrupt", b"new rcli binary", b"garbage!").await;

    sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"new rcli binary");
    assert!(!part.exists(), "the download is left behind");
}

/// A download already complete on disk is verified and installed without fetching it again (here the stored copy is
/// corrupted, so a second download would fail; it is large enough that the response starts before that shows).
#[tokio::test]
async fn a_complete_download_on_disk_is_installed_without_fetching_it_again() {
    let sys = TestSystem::start().await;
    let build = vec![7u8; 4 << 20];
    let (dir, part, target) = with_partial(&sys, "complete", &build, &build).await;
    sys.corrupt_stored_file(&build, &vec![8u8; 4 << 20]).await;

    sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert!(std::fs::read(&target).unwrap() == build, "the installed binary is not the build");
    assert!(!part.exists(), "the download is left behind");
}
