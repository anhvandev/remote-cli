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
    assert!(script.contains("rcli install --server"), "{script}");
    assert!(script.contains("--key"), "{script}");
    // Run as root: the build goes to a fresh private directory, never a fixed /tmp path another user could swap.
    assert!(script.contains("mktemp -d") && !script.contains("/tmp/rcli"), "{script}");
}

#[tokio::test]
async fn download_serves_the_rolled_out_build_and_unknown_targets_error() {
    let sys = TestSystem::start().await;
    for (version, build) in [("0.1.0", "old build"), ("0.2.0", "rolled out build"), ("0.3.0", "published, not rolled out")] {
        let signature = sys.sign_build(version, "linux-x86_64", build.as_bytes());
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
    assert!(script.contains("rcli install --server"), "{script}");
}

#[tokio::test]
async fn install_ps1_is_a_powershell_script_that_installs_the_agent() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.get_bytes("/install.ps1").await;
    assert_eq!(status, StatusCode::OK, "{:?}", String::from_utf8_lossy(&body));
    let script = String::from_utf8(body).unwrap();
    // Elevated or not, the same `rcli install`, and its output (the Device ID and Device Password) reaches this console.
    assert_eq!(script.matches(" install --server localhost").count(), 2, "{script}");
    assert!(!script.contains("agent"), "{script}");
    assert!(script.contains("Get-Content $log"), "{script}");
    assert!(script.contains("-Key"), "{script}");
    assert!(script.contains("download/windows/"), "{script}");
    // Installing a service needs Administrator: the one-liner relaunches itself through UAC and waits.
    assert!(script.contains("RunAs"), "{script}");
    // The Server Key comes in as -Key, never baked into a public script.
    assert!(!script.contains("rk_"), "{script}");
}

/// The `curl … | sudo sh` route end to end: the script downloads the rolled-out build and runs it as
/// `rcli install`, which prints the Device ID and Device Password, and `rcli connect` then works on that machine.
/// The build here is a stand-in that records its arguments; the test replays them through the real `rcli`, installing
/// under a temp directory (`rcli --root`) instead of `/`.
#[cfg(unix)]
#[tokio::test]
async fn a_scripted_install_makes_the_machine_a_device_that_can_connect() {
    let sys = TestSystem::start().await;
    sys.rcli(&["api-address", "set", &format!("http://{}", sys.server.http_addr)]).await.unwrap();
    // The script installs against the Server Address an Admin set (here with the test Agent port).
    let address = serde_json::json!({ "address": sys.agent_addr() });
    let (status, body) = sys.api.put_json("/api/v1/server-settings/server-address", &address).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let target = sys.start_agent("lab-pc").await;
    sys.wait_device(target.device_id(), |d| d["status"] == "Online").await;

    let machine = rcli_testkit::temp_dir("scripted");
    let args_file = rcli_testkit::temp_dir("scripted-args").join("args");
    let build = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", args_file.display());
    let target_name = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let signature = sys.sign_build("0.1.0", &target_name, build.as_bytes());
    let path = format!("/api/v1/agent-builds/0.1.0/{target_name}?signature={signature}");
    let (status, body) = sys.api.put_bytes(&path, build.into_bytes()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = sys.api.post_json("/api/v1/agent-builds/0.1.0/rollout", &serde_json::json!({ "all": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, script) = sys.api.with_token(None).get_bytes("/install.sh").await;
    let key = sys.server_key();
    let mut sh = tokio::process::Command::new("sh")
        .args(["-s", "--", "--key", &key])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(sh.stdin.as_mut().unwrap(), &script).await.unwrap();
    drop(sh.stdin.take());
    let run = sh.wait_with_output().await.unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));

    let recorded = std::fs::read_to_string(&args_file).unwrap();
    let args: Vec<&str> = recorded.lines().collect();
    assert_eq!(args, ["install", "--server", &sys.agent_addr(), "--key", &key]);
    let out = sys.rcli_in(&machine, &[&args[..], &["--name", "scripted"]].concat()).await.unwrap();
    assert!(out.contains("Device ID:") && out.contains("Device Password:"), "{out}");

    let password = target.device_password().unwrap();
    let out = sys.rcli_in(&machine, &["connect", target.device_id(), "--password", &password]).await.unwrap();
    assert!(out.contains(target.device_id()), "{out}");
    let out = sys.rcli_in(&machine, &["exec", target.device_id(), "--", "echo", "hi"]).await.unwrap();
    assert_eq!(out, "hi\n");
}

/// The Connect guide's .dmg/.deb/.rpm: an Admin uploads each package, anyone downloads it, a newer upload replaces it.
#[tokio::test]
async fn an_admin_publishes_install_packages_that_anyone_downloads() {
    let sys = TestSystem::start().await;
    let (status, _) = sys.api.with_token(None).get_bytes("/download/package/deb/x86_64").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing uploaded yet");

    let dir = rcli_testkit::temp_dir("packages");
    let deb = dir.join("rcli_0.1.0_amd64.deb");
    std::fs::write(&deb, b"first deb").unwrap();
    let out = sys.rcli(&["package", "publish", deb.to_str().unwrap(), "--arch", "x86_64"]).await.unwrap();
    assert!(out.contains("deb"), "{out}");
    std::fs::write(&deb, b"second deb").unwrap();
    sys.rcli(&["package", "publish", deb.to_str().unwrap(), "--arch", "x86_64"]).await.unwrap();
    let (status, bytes) = sys.api.with_token(None).get_bytes("/download/package/deb/x86_64").await;
    assert_eq!((status, bytes.as_slice()), (StatusCode::OK, b"second deb".as_slice()));

    // Only an Admin uploads, and only the package kinds the guide offers.
    let operator = sys.create_user("olga", "operator").await;
    let (status, _) = sys.api.with_token(Some(&operator)).put_bytes("/api/v1/packages/deb/x86_64", b"x".to_vec()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = sys.api.put_bytes("/api/v1/packages/zip/x86_64", b"x".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
