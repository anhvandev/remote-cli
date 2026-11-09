//! `rcli upgrade`: fetch the build rolled out for this OS/arch, verify it with the pinned update key, replace the
//! binary. The replacement path is a temp file here (the seam), never the running test binary.

use std::sync::atomic::{AtomicU32, Ordering};

use rcli_testkit::TestSystem;

fn temp_dir(name: &str) -> std::path::PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli13-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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
