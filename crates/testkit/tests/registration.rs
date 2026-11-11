use rcli_agent::ServerAccess;
use rcli_testkit::{FakePlatform, StatusCode, TestSystem};

#[tokio::test]
async fn a_device_registers_with_the_server_key_and_gets_a_nine_digit_device_id() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let id = agent.device_id();
    assert_eq!(id.len(), 9, "{id}");
    assert!(id.chars().all(|c| c.is_ascii_digit()), "{id}");

    let device = sys.wait_device(id, |d| d["status"] == "Online").await;
    assert_eq!(device["device_id"], id);
    assert_eq!(device["name"], "lab-pc");

    // The 9-digit Device ID addresses the Device through the Controller API.
    let (status, shown) = sys.api.get(&format!("/api/v1/devices/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["device_id"], id);

    let list = sys.rcli(&["devices"]).await.unwrap();
    let row = list.lines().find(|l| l.contains(id)).unwrap_or_else(|| panic!("no {id} in {list}"));
    assert!(row.contains("lab-pc") && row.contains("Online"), "{list}");
}

#[tokio::test]
async fn a_wrong_server_key_secret_is_refused_and_leaves_no_device() {
    let sys = TestSystem::start().await;
    let key = sys.server_key().to_owned();
    let (prefix, _) = key.rsplit_once('_').unwrap();
    let wrong = format!("{prefix}_{}", "0".repeat(32));
    let server_access = ServerAccess { addr: sys.agent_addr(), server_key: wrong };

    let err = rcli_agent::register(&server_access, &FakePlatform::new("impostor")).await.unwrap_err();
    assert!(err.to_string().contains("invalid_server_key"), "{err}");
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices, serde_json::json!([]), "nothing was registered");
}

#[tokio::test]
async fn a_server_key_with_another_fingerprint_refuses_the_server() {
    let sys = TestSystem::start().await;
    let key = sys.server_key().to_owned();
    let (_, secret) = key.rsplit_once('_').unwrap();
    let wrong = format!("rk_{}_{secret}", "00".repeat(32));
    let server_access = ServerAccess { addr: sys.agent_addr(), server_key: wrong };

    let err = rcli_agent::register(&server_access, &FakePlatform::new("lab-pc")).await.unwrap_err();
    assert!(err.to_string().contains("fingerprint"), "{err}");
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices, serde_json::json!([]), "nothing was sent to the Server");
}

#[tokio::test]
async fn controller_api_requires_admin_token() {
    let sys = TestSystem::start().await;
    for token in [None, Some("wrong")] {
        let (status, body) = sys.api.with_token(token).get("/api/v1/devices").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        let (status, _) = sys.api.with_token(token).get("/api/v1/server-key").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn admins_read_the_server_key_and_operators_are_refused() {
    let sys = TestSystem::start().await;
    let out = sys.rcli(&["server-key", "show"]).await.unwrap();
    let key = out.trim();
    assert!(key.starts_with("rk_"), "{out}");
    assert_eq!(key, sys.server_key(), "{out}");

    let op = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, body) = op.get("/api/v1/server-key").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let err = sys.rcli_as(&sys.create_user("op2", "operator").await, &["server-key", "show"]).await.unwrap_err();
    assert!(err.to_string().contains("admin_required"), "{err}");
}

#[tokio::test]
async fn a_device_is_online_while_connected_and_offline_30s_after_drop() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("build-box").await;

    let device = sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    assert_eq!(device["name"], "build-box");

    agent.stop();
    sys.wait_device(agent.device_id(), |d| !d["last_seen_at"].is_null()).await;
    sys.advance(29);
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    sys.advance(1);
    sys.wait_device(agent.device_id(), |d| d["status"] == "Offline").await;

    agent.start();
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online" && d["last_seen_at"].is_null()).await;
}
