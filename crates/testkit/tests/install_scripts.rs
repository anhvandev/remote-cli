//! Install scripts and build downloads served by the Server (ticket 17).
//! These are public: the one-line install has no credentials before registration.

use rcli_testkit::{StatusCode, TestSystem};

#[tokio::test]
async fn install_sh_is_a_shell_script_that_installs_the_agent() {
    let sys = TestSystem::start().await;
    // Public: no credentials at all (the one-line install has none before registration).
    let (status, body) = sys.api.with_token(None).get_bytes("/install.sh").await;
    assert_eq!(status, StatusCode::OK, "{:?}", String::from_utf8_lossy(&body));
    let script = String::from_utf8(body).unwrap();
    assert!(script.starts_with("#!/bin/sh"), "{script}");
    assert!(script.contains("rcli agent install"), "{script}");
    assert!(script.contains("--key"), "{script}");
    // Run as root: the build goes to a fresh private directory, never a fixed /tmp path another user could swap.
    assert!(script.contains("mktemp -d") && !script.contains("/tmp/rcli"), "{script}");
}

#[tokio::test]
async fn download_serves_the_rolled_out_build_and_unknown_targets_error() {
    let sys = TestSystem::start().await;
    // The Server stores, never checks, signatures; any 64-byte hex value will do here.
    let signature = "ab".repeat(64);
    for (version, build) in [("0.1.0", "old build"), ("0.2.0", "rolled out build"), ("0.3.0", "published, not rolled out")] {
        let (status, body) = sys
            .api
            .put_bytes(&format!("/api/v1/agent-builds/{version}/linux-x86_64?signature={signature}"), build.as_bytes().to_vec())
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    // Nothing rolled out to the whole fleet yet: a publish alone never reaches new installs.
    let (status, _) = sys.api.with_token(None).get_bytes("/download/linux/x86_64").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = sys.api.post_json("/api/v1/agent-builds/0.2.0/rollout", &serde_json::json!({ "all": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, bytes) = sys.api.with_token(None).get_bytes("/download/linux/x86_64").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"rolled out build");

    let (status, body) = sys.api.get_bytes("/download/unknown-os/unknown-arch").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        body["error"]["message"].as_str().unwrap().contains("unknown-os-unknown-arch"),
        "{body}"
    );
}

#[tokio::test]
async fn install_sh_downloads_from_the_origin_it_was_served_on() {
    let sys = TestSystem::start().await;
    let (_, body) = sys.api.get_bytes("/install.sh").await;
    let script = String::from_utf8(body).unwrap();
    let origin = format!("http://{}", sys.server.http_addr);
    assert!(script.contains(&format!("{origin}/download/")), "{script}");
    assert!(script.contains("agent install --server"), "{script}");
}

#[tokio::test]
async fn install_ps1_is_a_powershell_script_that_installs_the_agent() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.get_bytes("/install.ps1").await;
    assert_eq!(status, StatusCode::OK, "{:?}", String::from_utf8_lossy(&body));
    let script = String::from_utf8(body).unwrap();
    assert!(script.contains("agent install --server"), "{script}");
    assert!(script.contains("-Key"), "{script}");
    assert!(script.contains("download/windows/"), "{script}");
}
