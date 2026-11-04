use rcli_testkit::TestSystem;

#[tokio::test]
async fn rcli_devices_shows_device_status_as_text_and_json() {
    let sys = TestSystem::start().await;
    let code = sys.rcli(&["enrollment-code", "create"]).await.unwrap();
    let creds = sys.enroll(code.lines().next().unwrap(), &rcli_testkit::FakePlatform::new("lab-pc")).await.unwrap();
    let mut agent = rcli_testkit::TestAgent::new(sys.endpoint(), rcli_testkit::FakePlatform::new("lab-pc"), creds);
    agent.start();
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    let text = sys.rcli(&["devices"]).await.unwrap();
    let row = text.lines().find(|l| l.contains(agent.device_id())).expect(&text);
    assert!(row.contains("lab-pc") && row.contains("Online"), "{text}");

    let json: serde_json::Value = serde_json::from_str(&sys.rcli(&["devices", "--json"]).await.unwrap()).unwrap();
    assert_eq!(json[0]["id"], agent.device_id());
    assert_eq!(json[0]["name"], "lab-pc");
    assert_eq!(json[0]["status"], "Online");
    assert!(json[0]["last_seen_at"].is_null());
}

#[tokio::test]
async fn rcli_reports_typed_api_errors() {
    let sys = TestSystem::start().await;
    let base = format!("http://{}", sys.server.http_addr);
    let mut out = Vec::new();
    let err = rcli::run(["rcli", "--server", &base, "--token", "bad", "devices"], &mut out).await.unwrap_err();
    assert!(err.to_string().contains("unauthorized"), "{err}");
}
