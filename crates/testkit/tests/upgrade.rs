//! `rcli upgrade`: fetch the Latest Build for this OS/arch, check it against the Server's sha256, replace the
//! binary. The replacement path is a temp file here (the seam), never the running test binary.

use std::path::PathBuf;

use rcli_testkit::{temp_dir, TestSystem};

fn http_addr(sys: &TestSystem) -> String {
    format!("http://{}", sys.server.http_addr)
}

/// An installed machine upgrades itself with no key or other setup: the Server's sha256 is the only check.
#[tokio::test]
async fn an_installed_machine_upgrades_itself() {
    let sys = TestSystem::start().await;
    sys.rcli(&["api-address", "set", &http_addr(&sys)]).await.unwrap();
    let dir = temp_dir("controller");
    sys.install(&dir).await.unwrap();
    sys.upload_build("9.9.9", &rcli_agent::target(), b"new rcli binary").await;

    let target = temp_dir("controller-target").join("rcli");
    std::fs::write(&target, b"old rcli binary").unwrap();
    let out = sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap();

    assert!(out.contains("9.9.9"), "{out}");
    assert_eq!(std::fs::read(&target).unwrap(), b"new rcli binary");
}

/// The Latest Build is the highest semver version, whatever the upload order: 1.10.0 beats 1.9.0, and a
/// pre-release ranks below its release.
#[tokio::test]
async fn upgrade_fetches_the_latest_build() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("latest");
    sys.controller_config(&dir, None).await;
    let target = rcli_agent::target();
    for version in ["1.9.0", "1.10.0", "1.10.0-rc.1"] {
        sys.upload_build(version, &target, format!("build {version}").as_bytes()).await;
    }

    let path = temp_dir("latest-target").join("rcli");
    std::fs::write(&path, b"installed build").unwrap();
    let out = sys.rcli_in(&dir, &["upgrade", "--target", path.to_str().unwrap()]).await.unwrap();

    assert!(out.contains("1.10.0"), "{out}");
    assert_eq!(std::fs::read(&path).unwrap(), b"build 1.10.0");
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

/// The lookup `rcli upgrade` makes needs the Server Key.
#[tokio::test]
async fn the_build_lookup_needs_the_server_key() {
    let sys = TestSystem::start().await;
    sys.upload_build("1.0.0", &rcli_agent::target(), b"fleet build").await;
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
    assert!(!bytes.headers().contains_key("x-rcli-signature"));
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

/// Uploads `build` for a Controller set up in a new `name` dir, and leaves `part` as the partial download
/// `rcli upgrade` finds in its data dir. Returns the dir, the `.part` path and the binary to replace.
async fn with_partial(sys: &TestSystem, name: &str, build: &[u8], part: &[u8]) -> (PathBuf, PathBuf, PathBuf) {
    let dir = temp_dir(name);
    sys.controller_config(&dir, None).await;
    sys.upload_build("3.0.0", &rcli_agent::target(), build).await;
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

/// A download that never hashes to the Server's sha256 fails and leaves the old binary: the wrong partial download
/// resumes into a corrupted stored file, so neither the resume nor the fresh retry can match.
#[tokio::test]
async fn a_download_that_does_not_match_the_sha256_keeps_the_old_binary() {
    let sys = TestSystem::start().await;
    let (dir, _, target) = with_partial(&sys, "mismatch", b"new rcli binary", b"garbage!").await;
    sys.corrupt_stored_file(b"new rcli binary", b"bad rcli binary").await;

    sys.rcli_in(&dir, &["upgrade", "--target", target.to_str().unwrap()]).await.unwrap_err();

    assert_eq!(std::fs::read(&target).unwrap(), b"old rcli binary");
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
