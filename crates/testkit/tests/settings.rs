use rcli_agent::ServerAccess;
use rcli_testkit::{FakePlatform, StatusCode, TestSystem};

#[tokio::test]
async fn every_signed_in_user_reads_the_server_settings() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["server_key"], sys.server_key());
    assert_eq!(body["ca_fingerprint"].as_str().unwrap().len(), 64);
    assert_eq!(body["server_address"], "localhost");

    // The Connect guide shows the Server Key and Server Address to every signed-in User (ticket 18).
    let op = sys.api.with_token(Some(&sys.create_user("op").await));
    let (status, body) = op.get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["server_key"], sys.server_key());
    assert_eq!(body["server_address"], "localhost");

    // A password connection has no Server-admin rights and is refused (spec).
    let creds = sys.register(&FakePlatform::new("lab-pc")).await.unwrap();
    let (status, _) = sys.device_client(&creds.device_id, &creds.device_password).get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn replacing_the_server_key_keeps_registered_devices_and_retires_the_old_key() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let old_key = sys.rcli(&["server-key", "show"]).await.unwrap().trim().to_owned();

    let rotated: serde_json::Value =
        serde_json::from_str(&sys.rcli(&["server-key", "rotate", "--json"]).await.unwrap()).unwrap();
    let new_key = rotated["server_key"].as_str().unwrap();
    assert_ne!(new_key, old_key);
    // Only the secret changes; the CA fingerprint is the part that pins the Server.
    assert_eq!(new_key.split('_').nth(1), old_key.split('_').nth(1), "{new_key} {old_key}");
    assert_eq!(sys.rcli(&["server-key", "show"]).await.unwrap().trim(), new_key);

    // The registered Device keeps its certificate, so it is still Online.
    let device = sys.wait_device(agent.device_id(), |_| true).await;
    assert_eq!(device["status"], "Online");

    // The old key no longer registers a Device; the new one does.
    let old = ServerAccess { addr: sys.agent_addr(), server_key: old_key };
    let err = rcli_agent::register(&old, &FakePlatform::new("impostor")).await.unwrap_err();
    assert!(err.to_string().contains("invalid_server_key"), "{err}");
    let new = ServerAccess { addr: sys.agent_addr(), server_key: new_key.to_owned() };
    rcli_agent::register(&new, &FakePlatform::new("new-pc")).await.unwrap();
}

#[tokio::test]
async fn setting_the_server_address_reissues_the_certificate() {
    let sys = TestSystem::start().await;
    let port = sys.server.agent_addr.port();
    // Only `localhost` is in the certificate at first, so the Agent's pinning refuses another name.
    let by_ip = ServerAccess { addr: format!("127.0.0.1:{port}"), server_key: sys.server_key() };
    assert!(rcli_agent::register(&by_ip, &FakePlatform::new("lab-pc")).await.is_err());

    sys.rcli(&["server-address", "set", "127.0.0.1"]).await.unwrap();
    assert_eq!(sys.rcli(&["server-address", "show"]).await.unwrap().trim(), "127.0.0.1");
    // The certificate was reissued for the new name: registration through it now works.
    rcli_agent::register(&by_ip, &FakePlatform::new("lab-pc")).await.unwrap();
}

/// The Minimum Version is any semver (not checked against stored builds); empty means none.
#[tokio::test]
async fn the_minimum_version_is_set_and_cleared() {
    let sys = TestSystem::start().await;
    assert_eq!(sys.api.get("/api/v1/server-settings").await.1["minimum_version"], "");
    let api = &sys.api;
    let set = |v: &'static str| async move {
        api.put_json("/api/v1/server-settings/minimum-version", &serde_json::json!({ "version": v })).await
    };

    let (status, body) = set("1.2.0").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(sys.api.get("/api/v1/server-settings").await.1["minimum_version"], "1.2.0");

    let (status, body) = set("latest").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = set("").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(sys.api.get("/api/v1/server-settings").await.1["minimum_version"], "");
}

/// A Controller below the Minimum Version still runs its command and warns once on stderr; at or above it, or with
/// none set, it stays quiet.
#[tokio::test]
async fn a_controller_below_the_minimum_version_warns_once() {
    let sys = TestSystem::start().await;
    let set = |v: &'static str| {
        let api = &sys.api;
        async move { api.put_json("/api/v1/server-settings/minimum-version", &serde_json::json!({ "version": v })).await }
    };
    let devices = || async {
        let stderr = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let base = format!("http://{}", sys.server.http_addr);
        let argv = ["rcli", "--server", &base, "--token", &sys.admin_token, "devices"];
        rcli::run_with_input(argv, None, &mut Vec::new(), stderr.clone()).await.unwrap();
        let stderr = String::from_utf8(stderr.lock().unwrap().clone()).unwrap();
        stderr
    };

    assert_eq!(devices().await, "");
    set("0.0.1").await;
    assert_eq!(devices().await, "");

    set("999.0.0").await;
    assert_eq!(devices().await, "Server recommends rcli ≥ 999.0.0, run: rcli update\n");
}
