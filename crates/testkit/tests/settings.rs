use rcli_agent::ServerEndpoint;
use rcli_testkit::{FakePlatform, StatusCode, TestSystem};

#[tokio::test]
async fn admins_read_the_server_settings_and_operators_are_refused() {
    let sys = TestSystem::start().await;
    let (status, body) = sys.api.get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["server_key"], sys.server_key());
    assert_eq!(body["ca_fingerprint"].as_str().unwrap().len(), 64);
    assert_eq!(body["server_address"], "localhost");

    let op = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, body) = op.get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "admin_required");
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
    let old = ServerEndpoint { addr: sys.agent_addr(), server_key: old_key };
    let err = rcli_agent::register(&old, &FakePlatform::new("impostor")).await.unwrap_err();
    assert!(err.to_string().contains("invalid_server_key"), "{err}");
    let new = ServerEndpoint { addr: sys.agent_addr(), server_key: new_key.to_owned() };
    rcli_agent::register(&new, &FakePlatform::new("new-pc")).await.unwrap();
}

#[tokio::test]
async fn setting_the_server_address_reissues_the_certificate_and_is_refused_to_operators() {
    let sys = TestSystem::start().await;
    let port = sys.server.agent_addr.port();
    // Only `localhost` is in the certificate at first, so the Agent's pinning refuses another name.
    let by_ip = ServerEndpoint { addr: format!("127.0.0.1:{port}"), server_key: sys.server_key() };
    assert!(rcli_agent::register(&by_ip, &FakePlatform::new("lab-pc")).await.is_err());

    sys.rcli(&["server-address", "set", "127.0.0.1"]).await.unwrap();
    assert_eq!(sys.rcli(&["server-address", "show"]).await.unwrap().trim(), "127.0.0.1");
    // The certificate was reissued for the new name: registration through it now works.
    rcli_agent::register(&by_ip, &FakePlatform::new("lab-pc")).await.unwrap();

    let op = sys.create_user("op", "operator").await;
    for args in [vec!["server-address", "set", "example.com"], vec!["server-key", "rotate"]] {
        let err = sys.rcli_as(&op, &args).await.unwrap_err();
        assert!(err.to_string().contains("admin_required"), "{err}");
    }
    let (status, _) = sys.api.with_token(Some(&op)).post("/api/v1/server-settings/server-key/rotate").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
