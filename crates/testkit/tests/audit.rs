use rcli_testkit::{FakePlatform, LiveViewer, SessionState, StatusCode, TestSystem};
use serde_json::{json, Value};

const DAY: i64 = 24 * 3600;

async fn audit(sys: &TestSystem, query: &str) -> Vec<Value> {
    let (status, body) = sys.api.get(&format!("/api/v1/audit{query}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

#[tokio::test]
async fn every_action_is_recorded_and_filterable_by_admins_only() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let other = sys.start_agent("other-pc").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;
    let alice_token = sys.create_user("alice", "operator").await;
    let alice = sys.api.with_token(Some(&alice_token));
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));
    alice.post(&format!("/api/v1/devices/{dev}/lock")).await;

    let click = json!({ "actions": [{"type": "click", "x": 10, "y": 20}], "observe": "shot" });
    let (status, obs) = alice.post_json(&format!("/api/v1/devices/{dev}/actions"), &click).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    sys.advance(3600);
    let (status, _) = bob.post_json(&format!("/api/v1/devices/{dev}/actions"), &json!({ "actions": [] })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bob.post_json(&format!("/api/v1/devices/{}/actions", other.device_id()), &json!({})).await;
    assert_eq!(status, StatusCode::OK);

    let entries = audit(&sys, "?device=lab-pc").await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    let first = &entries[0];
    assert_eq!(first["user"], "alice");
    assert_eq!(first["device_id"], dev);
    assert_eq!(first["device"], "lab-pc");
    assert_eq!(first["action"], "act");
    assert_eq!(first["params"]["actions"][0], json!({"type": "click", "x": 10, "y": 20}));
    assert_eq!(first["result"]["error"], Value::Null);
    assert!(first["at"].is_string());
    // Observations are relayed, never stored.
    assert!(!first.to_string().contains(obs["images"][0]["data"].as_str().unwrap()));
    assert_eq!(entries[1]["user"], "bob");

    assert_eq!(audit(&sys, "?user=bob").await.len(), 2);
    let until = audit(&sys, &format!("?until={}", first["at"].as_str().unwrap())).await;
    assert_eq!(until.len(), 1);
    let since = audit(&sys, &format!("?since={}", entries[1]["at"].as_str().unwrap())).await;
    assert_eq!(since.len(), 2);

    let (status, body) = alice.get("/api/v1/audit").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let out = sys.rcli(&["audit", "--device", "lab-pc", "--user", "alice"]).await.unwrap();
    assert!(out.contains("alice") && out.contains("click"), "{out}");
    assert_eq!(out.lines().count(), 2, "header + one entry: {out}");
}

#[tokio::test]
async fn failed_actions_are_recorded_with_their_error() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;

    let bad = json!({ "actions": [{"type": "click", "x": 99999, "y": 0}] });
    let (status, _) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let entries = audit(&sys, "").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["result"]["error"], "invalid_action");
}

#[tokio::test]
async fn secret_text_and_text_typed_at_a_lock_screen_are_masked() {
    let sys = TestSystem::start().await;
    let platform = FakePlatform::new("lab-pc");
    let agent = sys.start_agent_on(platform.clone()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");
    let typed = |text: &str, secret: bool| {
        json!({ "actions": [{"type": "type", "text": text, "secret": secret}], "observe": "none" })
    };

    sys.api.post_json(&actions, &typed("hello", false)).await;
    let (status, body) = sys.api.post_json(&actions, &typed("hunter2", true)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    platform.set_session_state(SessionState::Locked);
    sys.wait_device_details(dev, |d| d["resources"]["session"] == "LockScreen").await;
    sys.api.post_json(&actions, &typed("s3cret", false)).await;

    let entries = audit(&sys, "").await;
    let texts: Vec<&Value> = entries.iter().map(|e| &e["params"]["actions"][0]["text"]).collect();
    assert_eq!(texts, [&json!("hello"), &json!("***"), &json!("***")]);
    let all = serde_json::to_string(&entries).unwrap();
    assert!(!all.contains("hunter2") && !all.contains("s3cret"), "{all}");
}

#[tokio::test]
async fn entries_older_than_90_days_are_removed() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let observe = json!({ "actions": [], "observe": "none" });
    sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &observe).await;

    sys.advance(90 * DAY - 60);
    assert_eq!(audit(&sys, "").await.len(), 1);
    sys.advance(120);
    assert_eq!(audit(&sys, "").await.len(), 0);
}

#[tokio::test]
async fn admin_operations_and_live_views_are_recorded_without_the_unlock_password() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let old = sys.register(&FakePlatform::new("old-pc")).await.unwrap();
    let doomed = sys.register(&FakePlatform::new("doomed")).await.unwrap();
    let path = format!("/api/v1/devices/{dev}");

    let live = LiveViewer::open(&sys.api, &dev, false).await.expect("open live view");
    assert_eq!(live.close(&sys.api).await, StatusCode::NO_CONTENT);
    let ok = |(status, body): (StatusCode, Value)| assert!(status.is_success(), "{body}");
    ok(sys.api.post_json(&format!("{path}/unlock-password"), &json!({ "password": "hunter2" })).await);
    ok(sys.api.post(&format!("{path}/unlock-password/clear")).await);
    ok(sys.api.patch_json(&path, &json!({ "name": "lab-pc-2" })).await);
    ok(sys.api.post_json(&format!("{path}/merge"), &json!({ "from": old.device_id })).await);
    ok(sys.api.post(&format!("{path}/revoke")).await);
    ok(sys.api.delete(&format!("/api/v1/devices/{}", doomed.device_id)).await);

    let entries = audit(&sys, "").await;
    assert!(!Value::from(entries.clone()).to_string().contains("hunter2"), "{entries:?}");
    let got: Vec<_> = entries
        .iter()
        .map(|e| (e["user"].as_str().unwrap(), e["device_id"].as_str().unwrap(), e["action"].as_str().unwrap(), e["params"].clone(), e["result"]["error"].clone()))
        .collect();
    let session = &entries[0]["result"]["session"];
    assert!(session.is_string(), "{entries:?}");
    let doomed_id = doomed.device_id.to_string();
    assert_eq!(
        got,
        vec![
            ("admin", &dev[..], "live.start", json!({ "display": null }), Value::Null),
            ("admin", &dev[..], "unlock_password.set", json!({}), Value::Null),
            ("admin", &dev[..], "unlock_password.clear", json!({}), Value::Null),
            ("admin", &dev[..], "device.rename", json!({ "name": "lab-pc-2" }), Value::Null),
            ("admin", &dev[..], "device.merge", json!({ "from": old.device_id }), Value::Null),
            ("admin", &dev[..], "device.revoke", json!({}), Value::Null),
            ("admin", &doomed_id[..], "device.delete", json!({ "name": "doomed" }), Value::Null),
        ]
    );
}
