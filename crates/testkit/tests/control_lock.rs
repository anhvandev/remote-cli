//! Control Lock rules: idle timeout, disconnect grace, no pre-emption, admin force-release, on-Device indicator.

use rcli_testkit::{eventually, platform::Platform, StatusCode, TestSystem};
use rcli_server::clock::Clock;
use serde_json::{json, Value};

fn lock(dev: &str) -> String {
    format!("/api/v1/devices/{dev}/lock")
}

fn actions(dev: &str) -> String {
    format!("/api/v1/devices/{dev}/actions")
}

fn click() -> Value {
    json!({ "actions": [{"type": "click", "x": 10, "y": 10}], "observe": "none" })
}

fn time(v: &Value) -> chrono::DateTime<chrono::Utc> {
    v.as_str().unwrap_or_else(|| panic!("not a time: {v}")).parse().unwrap()
}

#[tokio::test]
async fn any_action_from_the_holder_resets_the_ten_minute_idle_timer() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob").await));

    let (status, l) = alice.post(&lock(dev)).await;
    assert_eq!(status, StatusCode::OK, "{l}");
    assert_eq!(time(&l["expires_at"]) - time(&l["acquired_at"]), chrono::Duration::minutes(10));

    // 9 minutes, then an observe-only request (no Actions) keeps the lock alive.
    sys.advance(9 * 60);
    let (status, _) = alice.post_json(&actions(dev), &json!({ "actions": [], "observe": "none" })).await;
    assert_eq!(status, StatusCode::OK);
    sys.advance(9 * 60);
    let (_, l) = bob.get(&lock(dev)).await;
    assert_eq!(l["holder"], "alice", "18 min after acquire, 9 min after the last Action");

    // Bob is refused and told who holds it and until when.
    let (status, body) = bob.post(&lock(dev)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["holder"], "alice");
    assert_eq!(body["error"]["expires_at"], l["expires_at"]);
    let (_, body) = bob.post_json(&actions(dev), &click()).await;
    assert_eq!((body["error"]["code"].clone(), body["error"]["expires_at"].clone()), (json!("control_lock_held"), l["expires_at"].clone()));

    // An input Action resets it too.
    let (status, _) = alice.post_json(&actions(dev), &click()).await;
    assert_eq!(status, StatusCode::OK);
    sys.advance(10 * 60 - 1);
    let (_, l) = bob.get(&lock(dev)).await;
    assert_eq!(l["holder"], "alice");

    // 10 minutes without any Action: released, Bob can take it.
    sys.advance(1);
    let (_, l) = bob.get(&lock(dev)).await;
    assert_eq!(l["holder"], Value::Null);
    let (status, _) = alice.post_json(&actions(dev), &click()).await;
    assert_eq!(status, StatusCode::CONFLICT, "the old holder lost it");
    let (status, l) = bob.post(&lock(dev)).await;
    assert_eq!((status, l["holder"].clone()), (StatusCode::OK, json!("bob")));
}

#[tokio::test]
async fn lock_survives_a_short_holder_disconnect_and_ends_after_60_seconds() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob").await));
    alice.post(&lock(dev)).await;

    // Alice's connection: open, then drop. The Server notices, then 59 s pass.
    let conn = alice.watch(&format!("{}/watch", lock(dev))).await;
    drop(conn);
    let gone = eventually(|| async {
        let (_, l) = bob.get(&lock(dev)).await;
        (l["expires_at"] != Value::Null && time(&l["expires_at"]) - sys.clock.now() <= chrono::Duration::seconds(60)).then_some(l)
    })
    .await;
    assert_eq!(time(&gone["expires_at"]) - sys.clock.now(), chrono::Duration::seconds(60));
    sys.advance(59);
    // She reconnects in time: the lock is hers, and only the idle timer counts again.
    let conn = alice.watch(&format!("{}/watch", lock(dev))).await;
    let (_, l) = bob.get(&lock(dev)).await;
    assert_eq!(l["holder"], "alice");
    assert!(time(&l["expires_at"]) - sys.clock.now() > chrono::Duration::seconds(60), "{l}");

    // Second drop, 60 s later it is released.
    drop(conn);
    eventually(|| async {
        let (_, l) = bob.get(&lock(dev)).await;
        (time(&l["expires_at"]) - sys.clock.now() <= chrono::Duration::seconds(60)).then_some(())
    })
    .await;
    sys.advance(60);
    let (_, l) = bob.get(&lock(dev)).await;
    assert_eq!(l["holder"], Value::Null);
}

#[tokio::test]
async fn force_release_is_audited_and_others_cannot_pre_empt() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob").await));
    alice.post(&lock(dev)).await;

    // Nobody else can take a held lock; force-release is the way to recover it.
    let (status, body) = sys.api.post(&lock(dev)).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::CONFLICT, json!("control_lock_held")));

    // Non-holders still observe and read metrics.
    let (status, _) = bob.post_json(&actions(dev), &json!({ "actions": [] })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bob.get(&format!("/api/v1/devices/{dev}")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bob.get(&format!("/api/v1/devices/{dev}/processes")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, l) = sys.api.post(&format!("{}/force-release", lock(dev))).await;
    assert_eq!((status, l["holder"].clone()), (StatusCode::OK, Value::Null));
    let (status, _) = alice.post_json(&actions(dev), &click()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, l) = bob.post(&lock(dev)).await;
    assert_eq!((status, l["holder"].clone()), (StatusCode::OK, json!("bob")));

    let (_, log) = sys.api.get("/api/v1/audit").await;
    let entry = log.as_array().unwrap().iter().find(|e| e["action"] == "lock.force_release").expect("audited");
    assert_eq!(entry["user"], "admin");
    assert_eq!(entry["device_id"], dev);
    assert_eq!(entry["params"]["holder"], "alice");
}

#[tokio::test]
async fn device_shows_the_control_indicator_while_the_lock_is_held() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let p = agent.platform.clone();
    let shown = |on: bool| {
        let p = p.clone();
        eventually(move || {
            let p = p.clone();
            async move { (p.control_indicator() == on).then_some(()) }
        })
    };
    assert!(!p.control_indicator());

    sys.api.post(&lock(&dev)).await;
    shown(true).await;
    sys.api.post(&format!("{}/release", lock(&dev))).await;
    shown(false).await;

    // Expiry hides it too; a reconnecting Agent is told the current state.
    sys.api.post(&lock(&dev)).await;
    shown(true).await;
    agent.stop();
    p.set_control_indicator(false).unwrap();
    agent.start();
    shown(true).await;
    sys.advance(10 * 60);
    shown(false).await;
}

#[tokio::test]
async fn cli_shows_expiry_and_admin_force_releases() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.create_user("alice").await;
    sys.rcli_as(&alice, &["lock", "acquire", dev]).await.unwrap();

    let err = sys.rcli(&["lock", "acquire", dev]).await.unwrap_err().to_string();
    assert!(err.contains("alice") && err.contains("expires"), "{err}");
    let out = sys.rcli(&["lock", "status", dev]).await.unwrap();
    assert!(out.contains("held by alice") && out.contains("expires"), "{out}");
    sys.rcli(&["lock", "force-release", dev]).await.unwrap();
    let out = sys.rcli(&["lock", "status", dev]).await.unwrap();
    assert!(out.contains("free"), "{out}");
}
