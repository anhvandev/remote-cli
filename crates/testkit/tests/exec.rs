use std::time::{Duration, Instant};

use rcli_testkit::{FakePlatform, SessionState, StatusCode, TestSystem};
use serde_json::{json, Value};

async fn audit(sys: &TestSystem) -> Vec<Value> {
    let (status, body) = sys.api.get("/api/v1/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().iter().filter(|e| e["action"] == "terminal").cloned().collect()
}

#[tokio::test]
async fn exec_returns_output_and_exit_code_as_the_logged_in_user_without_the_lock() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let alice_token = sys.create_user("alice", "operator").await;
    let alice = sys.api.with_token(Some(&alice_token));
    // Someone else holds the Control Lock: the terminal is not exclusive.
    let (status, _) = sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status, StatusCode::OK);

    let path = format!("/api/v1/devices/{dev}/exec");
    let (status, out) = alice.post_json(&path, &json!({ "command": "echo hi" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out, json!({ "exit_code": 0, "stdout": "hi\n", "stderr": "", "timed_out": false, "as": "user" }));

    let (_, out) = alice.post_json(&path, &json!({ "command": "whoami" })).await;
    assert_eq!(out["stdout"], "user\n", "{out}");

    let (_, out) = alice.post_json(&path, &json!({ "command": "nope" })).await;
    assert_eq!(out["exit_code"], 127, "{out}");
    assert_eq!(out["stderr"], "nope: not found\n");

    let stdout = sys.rcli_as(&alice_token, &["exec", "lab-pc", "--", "echo", "from", "cli"]).await.unwrap();
    assert_eq!(stdout, "from cli\n");
    let err = sys.rcli_as(&alice_token, &["exec", "lab-pc", "--", "nope"]).await.unwrap_err();
    assert!(format!("{err:#}").contains("127"), "{err:#}");

    let entries = audit(&sys).await;
    assert_eq!(entries.len(), 5, "{entries:?}");
    assert_eq!(entries[0]["user"], "alice");
    assert_eq!(entries[0]["params"]["command"], "echo hi");
    assert_eq!(entries[0]["result"]["exit_code"], 0);
    assert_eq!(entries[0]["result"]["as"], "user");
    assert_eq!(entries[2]["result"]["exit_code"], 127);
}

#[tokio::test]
async fn exec_timeout_is_enforced() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;

    let started = Instant::now();
    let body = json!({ "command": "sleep 30", "timeout_secs": 1 });
    let (status, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &body).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(out["timed_out"], true, "{out}");
    assert_eq!(out["exit_code"], Value::Null);

    let entries = audit(&sys).await;
    assert_eq!(entries[0]["result"]["timed_out"], true, "{entries:?}");

    let err = sys.rcli(&["exec", "lab-pc", "--timeout", "1", "--", "sleep", "30"]).await.unwrap_err();
    assert!(format!("{err:#}").contains("timed out"), "{err:#}");
}

#[tokio::test]
async fn root_is_not_gated_by_server_roles() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let alice_token = sys.create_user("alice", "operator").await;
    let path = format!("/api/v1/devices/{dev}/exec");
    let as_root = json!({ "command": "whoami", "as": "root" });

    // The Device's own Device Settings decide root access (ticket 07), not the Server role.
    let (status, body) = sys.api.with_token(Some(&alice_token)).post_json(&path, &as_root).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["as"], "root");
    let out = sys.rcli_as(&alice_token, &["exec", "lab-pc", "--as", "root", "--", "whoami"]).await.unwrap();
    assert_eq!(out, "root\n");

    let entries = audit(&sys).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0]["user"], "alice");
    assert_eq!(entries[0]["params"]["as"], "root");
}

#[tokio::test]
async fn with_nobody_logged_in_any_caller_runs_commands_as_root() {
    let sys = TestSystem::start().await;
    let platform = FakePlatform::new("lab-pc");
    platform.set_session_state(SessionState::LoginScreen);
    let agent = sys.start_agent_on(platform).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice", "operator").await));
    let path = format!("/api/v1/devices/{dev}/exec");

    let (status, out) = alice.post_json(&path, &json!({ "command": "whoami" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["stdout"], "root\n");
    assert_eq!(out["as"], "root");

    let (status, out) = sys.api.post_json(&path, &json!({ "command": "whoami" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["as"], "root");
}
