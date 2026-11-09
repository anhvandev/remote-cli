use rcli_testkit::{eventually, TestSystem};

/// A fresh temp directory.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("rcli-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn rcli_agent_run_registers_and_connects_a_device() {
    let sys = TestSystem::start().await;
    let created = sys.rcli(&["enrollment-code", "create"]).await.unwrap();
    let (code, pin) = (created.lines().next().unwrap(), created.lines().nth(1).unwrap().trim_start_matches("ca-fingerprint "));
    let dir = temp_dir("agent-run");
    let ca = dir.join("ca.pem");
    std::fs::write(&ca, &sys.server.ca_pem).unwrap();
    let argv = [
        "rcli", "agent", "run",
        "--server", &format!("localhost:{}", sys.server.agent_addr.port()),
        "--ca", ca.to_str().unwrap(),
        "--ca-fingerprint", pin,
        "--state-dir", dir.to_str().unwrap(),
        "--name", "lab-pc",
        "--enroll", code,
    ];
    let mut out = Vec::new();
    let agent = rcli::run(argv, &mut out);
    tokio::pin!(agent);
    let online = async {
        eventually(|| async {
            let (_, devices) = sys.api.get("/api/v1/devices").await;
            devices.as_array()?.iter().find(|d| d["name"] == "lab-pc" && d["status"] == "Online").cloned()
        })
        .await
    };
    let device = tokio::select! {
        r = &mut agent => panic!("the Agent exited: {r:?}"),
        d = online => d,
    };
    assert_eq!(device["agent_version"], "0.1.0");
    assert!(dir.join("credentials.json").exists(), "the Agent keeps its Device credentials");
}

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
