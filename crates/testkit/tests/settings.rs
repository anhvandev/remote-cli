use rcli_testkit::{StatusCode, TestSystem};

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
