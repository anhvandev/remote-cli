//! Power Actions: Restart, Shut down, Sleep (need the Control Lock holder or an admin while held), Lock screen (no lock).

use rcli_testkit::{platform::PowerAction, StatusCode, TestSystem};
use serde_json::{json, Value};

fn power(dev: &str) -> String {
    format!("/api/v1/devices/{dev}/power")
}

#[tokio::test]
async fn an_operator_restarts_a_device_and_it_is_audited() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice", "operator").await));

    let (status, body) = alice.post_json(&power(dev), &json!({ "action": "restart" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "action": "restart" }));
    assert_eq!(agent.platform.power_actions(), vec![PowerAction::Restart]);

    let (_, entries) = sys.api.get("/api/v1/audit").await;
    let e = entries.as_array().unwrap().iter().find(|e| e["action"] == "power.restart").expect("audited");
    assert_eq!((e["user"].clone(), e["result"].clone()), (json!("alice"), json!({ "error": null })));
}

#[tokio::test]
async fn while_a_control_lock_is_held_only_the_holder_or_an_admin_may_restart_shut_down_or_sleep() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice", "operator").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));
    let (status, _) = alice.post(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status, StatusCode::OK);

    for action in ["restart", "shutdown", "sleep"] {
        let (status, body) = bob.post_json(&power(dev), &json!({ "action": action })).await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {body}");
        assert_eq!((body["error"]["code"].clone(), body["error"]["holder"].clone()), (json!("control_lock_held"), json!("alice")));
    }
    assert!(agent.platform.power_actions().is_empty());

    // Lock screen needs no Control Lock.
    let (status, body) = bob.post_json(&power(dev), &json!({ "action": "lock" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = alice.post_json(&power(dev), &json!({ "action": "sleep" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = sys.api.post_json(&power(dev), &json!({ "action": "shutdown" })).await;
    assert_eq!(status, StatusCode::OK, "admin: {body}");
    assert_eq!(agent.platform.power_actions(), vec![PowerAction::Lock, PowerAction::Sleep, PowerAction::Shutdown]);

    // Refusals are not audited; every power Action that ran is.
    let (_, entries) = sys.api.get("/api/v1/audit").await;
    let power: Vec<(Value, Value)> = entries.as_array().unwrap().iter()
        .filter(|e| e["action"].as_str().unwrap().starts_with("power."))
        .map(|e| (e["user"].clone(), e["action"].clone()))
        .collect();
    assert_eq!(power, vec![
        (json!("bob"), json!("power.lock")),
        (json!("alice"), json!("power.sleep")),
        (json!("admin"), json!("power.shutdown")),
    ]);
}

#[tokio::test]
async fn without_a_control_lock_anyone_may_power_off_and_bad_actions_are_rejected() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));

    let (status, body) = bob.post_json(&power(dev), &json!({ "action": "hibernate" })).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("bad_request")));
    let (status, body) = bob.post_json(&power(dev), &json!({ "action": "shutdown" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(agent.platform.power_actions(), vec![PowerAction::Shutdown]);
}

#[tokio::test]
async fn rcli_power_runs_the_action_and_prints_the_lock_holder_when_refused() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let alice = sys.create_user("alice", "operator").await;
    let bob = sys.create_user("bob", "operator").await;

    let out = sys.rcli_as(&bob, &["power", "lab-pc", "lock"]).await.unwrap();
    assert_eq!(out, "lab-pc: lock sent\n");
    sys.rcli_as(&alice, &["lock", "acquire", "lab-pc"]).await.unwrap();
    let err = sys.rcli_as(&bob, &["power", "lab-pc", "restart"]).await.unwrap_err();
    assert!(err.to_string().contains("alice"), "{err}");
    assert_eq!(agent.platform.power_actions(), vec![PowerAction::Lock]);
}
