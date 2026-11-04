use rcli_testkit::{FakePlatform, StatusCode, TestSystem};

#[tokio::test]
async fn enrolled_device_is_online_while_connected_and_offline_30s_after_drop() {
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

#[tokio::test]
async fn enrollment_code_is_single_use() {
    let sys = TestSystem::start().await;
    let code = sys.create_enrollment_code().await;
    sys.enroll(&code, &FakePlatform::new("first")).await.unwrap();

    let err = sys.enroll(&code, &FakePlatform::new("second")).await.unwrap_err();
    assert!(err.to_string().contains("invalid_enrollment_code"), "{err}");
    let err = sys.enroll("not-a-code", &FakePlatform::new("third")).await.unwrap_err();
    assert!(err.to_string().contains("invalid_enrollment_code"), "{err}");

    let (_, devices) = sys.api.get("/api/v1/devices").await;
    let names: Vec<_> = devices.as_array().unwrap().iter().map(|d| d["name"].clone()).collect();
    assert_eq!(names, ["first"]);
}

#[tokio::test]
async fn controller_api_requires_admin_token() {
    let sys = TestSystem::start().await;
    for token in [None, Some("wrong")] {
        let (status, body) = sys.api.with_token(token).get("/api/v1/devices").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        let (status, _) = sys.api.with_token(token).post("/api/v1/enrollment-codes").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
