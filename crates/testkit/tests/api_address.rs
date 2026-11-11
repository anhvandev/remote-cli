//! The API Address: the Server learns it from an Admin's Dashboard visit over `https://` and hands it to Agents.

use rcli_testkit::{eventually, temp_dir, ApiClient, FakePlatform, StatusCode, TestSystem};

async fn api_address(sys: &TestSystem) -> String {
    let (status, body) = sys.api.get("/api/v1/server-settings").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["api_address"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn an_admins_https_visit_through_the_proxy_sets_the_api_address() {
    let sys = TestSystem::start().await;
    assert_eq!(api_address(&sys).await, "", "nothing is learned before an https visit");

    let (status, body) = sys.api.via_proxy("https", "rcli.example.com").get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(api_address(&sys).await, "https://rcli.example.com");
}

#[tokio::test]
async fn an_admin_corrects_it_and_later_visits_keep_the_correction() {
    let sys = TestSystem::start().await;
    sys.api.via_proxy("https", "old.example.com").get("/api/v1/devices").await;
    assert_eq!(sys.rcli(&["api-address", "show"]).await.unwrap().trim(), "https://old.example.com");

    sys.rcli(&["api-address", "set", "https://rcli.example.com:8443"]).await.unwrap();
    assert_eq!(sys.rcli(&["api-address", "show"]).await.unwrap().trim(), "https://rcli.example.com:8443");
    sys.api.via_proxy("https", "old.example.com").get("/api/v1/devices").await;
    assert_eq!(api_address(&sys).await, "https://rcli.example.com:8443", "a visit never overrides a correction");

    // Only an https:// origin is an API Address.
    for bad in ["http://rcli.example.com", "rcli.example.com", "https://rcli.example.com/path", ""] {
        let (status, body) = sys.api.put_json("/api/v1/server-settings/api-address", &serde_json::json!({ "address": bad })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
    }
    assert_eq!(api_address(&sys).await, "https://rcli.example.com:8443");

    let op = sys.create_user("op", "operator").await;
    let err = sys.rcli_as(&op, &["api-address", "set", "https://evil.example.com"]).await.unwrap_err();
    assert!(err.to_string().contains("admin_required"), "{err}");
    assert_eq!(api_address(&sys).await, "https://rcli.example.com:8443");
}

#[tokio::test]
async fn a_registering_agent_gets_it_and_a_connected_agent_gets_a_correction_on_its_next_connection() {
    let sys = TestSystem::start().await;
    let creds = sys.register(&FakePlatform::new("early")).await.unwrap();
    assert_eq!(creds.api_address, "", "an install before any https visit still registers, with no API Address");

    sys.api.via_proxy("https", "rcli.example.com").get("/api/v1/devices").await;
    let creds = sys.register(&FakePlatform::new("lab-pc")).await.unwrap();
    assert_eq!(creds.api_address, "https://rcli.example.com");

    let dir = temp_dir("api-address");
    let mut agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    let info = || async {
        let out = sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.ok()?;
        serde_json::from_str::<serde_json::Value>(&out).ok()
    };
    eventually(|| async { info().await.filter(|i| i["api_address"] == "https://rcli.example.com") }).await;

    sys.rcli(&["api-address", "set", "https://moved.example.com"]).await.unwrap();
    agent.stop();
    agent.start();
    eventually(|| async { info().await.filter(|i| i["api_address"] == "https://moved.example.com") }).await;
}

#[tokio::test]
async fn http_internal_hosts_operators_and_untrusted_peers_never_set_it() {
    let sys = TestSystem::start_dual_stack().await;
    for (proto, host) in [
        ("http", "rcli.example.com"),
        ("https", "localhost:3000"),
        ("https", "127.0.0.1"),
        ("https", "192.168.1.20:8443"),
        ("https", "10.0.0.5"),
        ("https", "[::1]:3000"),
        ("https", "[fd00::1]"),
    ] {
        let (status, body) = sys.api.via_proxy(proto, host).get("/api/v1/devices").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(api_address(&sys).await, "", "{proto}://{host} must not set it");
    }

    // Only an Admin's visit counts.
    let op = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    op.via_proxy("https", "rcli.example.com").get("/api/v1/devices").await;
    assert_eq!(api_address(&sys).await, "");

    // The test Server trusts only 127.0.0.1 as a proxy; the same headers from `::1` are the client's own word.
    let untrusted = ApiClient::new(format!("http://[::1]:{}", sys.server.http_addr.port()), Some(&sys.admin_token));
    let (status, body) = untrusted.via_proxy("https", "rcli.example.com").get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(api_address(&sys).await, "");
}
