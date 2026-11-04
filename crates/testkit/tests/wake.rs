use rcli_testkit::{FakePlatform, StatusCode, TestAgent, TestSystem, platform::NetworkInterface};
use serde_json::Value;

const TARGET_MAC: [u8; 6] = [0xaa, 0xbb, 0xcc, 0x00, 0x00, 0x01];

fn on_lan(hostname: &str, mac: &str, ip: &str) -> FakePlatform {
    let p = FakePlatform::new(hostname);
    p.update(|s| {
        s.inventory.network_interfaces = vec![
            NetworkInterface { name: "lo".into(), mac: "00:00:00:00:00:00".into(), ips: vec!["127.0.0.1/8".into()] },
            NetworkInterface { name: "eth0".into(), mac: mac.into(), ips: vec![ip.into(), "fe80::1/64".into()] },
        ]
    });
    p
}

/// Starts `target` once so the Server learns its inventory, then takes it Offline.
async fn offline_target(sys: &TestSystem) -> TestAgent {
    let mut target = sys.start_agent_on(on_lan("target", "AA:BB:CC:00:00:01", "192.168.1.20/24")).await;
    let id = target.device_id().to_owned();
    sys.wait_device_details(&id, |d| !d["inventory"].is_null()).await;
    target.stop();
    sys.wait_device(&id, |d| !d["last_seen_at"].is_null()).await;
    sys.advance(31);
    sys.wait_device(&id, |d| d["status"] == "Offline").await;
    target
}

async fn wake(sys: &TestSystem, id: &str) -> (StatusCode, Value) {
    sys.api.post(&format!("/api/v1/devices/{id}/wake")).await
}

async fn audit_wakes(sys: &TestSystem) -> Vec<Value> {
    let (_, entries) = sys.api.get("/api/v1/audit").await;
    entries.as_array().unwrap().iter().filter(|e| e["action"] == "wake").cloned().collect()
}

#[tokio::test]
async fn a_relay_on_the_targets_subnet_sends_the_packet_and_the_device_is_waking_until_online() {
    let sys = TestSystem::start().await;
    let elsewhere = sys.start_agent_on(on_lan("elsewhere", "02:00:00:00:00:09", "10.0.0.5/24")).await;
    let relay = sys.start_agent_on(on_lan("relay", "02:00:00:00:00:05", "192.168.1.5/24")).await;
    for a in [&elsewhere, &relay] {
        sys.wait_device_details(a.device_id(), |d| !d["inventory"].is_null()).await;
    }
    let mut target = offline_target(&sys).await;
    let id = target.device_id().to_owned();

    let (status, body) = wake(&sys, &id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["relay"]["name"], "relay");
    assert_eq!(body["mac"], "aa:bb:cc:00:00:01");
    assert_eq!(relay.platform.wakes_sent(), vec![TARGET_MAC]);
    assert!(elsewhere.platform.wakes_sent().is_empty());
    sys.wait_device(&id, |d| d["status"] == "Waking" && d["wake_error"].is_null()).await;

    sys.advance(170);
    target.start();
    sys.wait_device(&id, |d| d["status"] == "Online").await;
    sys.advance(60);
    sys.wait_device(&id, |d| d["status"] == "Online" && d["wake_error"].is_null()).await;

    let entries = audit_wakes(&sys).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["device_id"], id);
    assert_eq!(entries[0]["user"], "admin");
    assert_eq!(entries[0]["result"]["relay"], "relay");
    assert_eq!(entries[0]["result"]["error"], Value::Null);
}

#[tokio::test]
async fn a_device_that_does_not_come_online_within_3_minutes_is_offline_with_wake_failed() {
    let sys = TestSystem::start().await;
    let relay = sys.start_agent_on(on_lan("relay", "02:00:00:00:00:05", "192.168.1.5/24")).await;
    sys.wait_device_details(relay.device_id(), |d| !d["inventory"].is_null()).await;
    let target = offline_target(&sys).await;
    let id = target.device_id().to_owned();

    let (status, body) = wake(&sys, &id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sys.advance(179);
    sys.wait_device(&id, |d| d["status"] == "Waking").await;
    sys.advance(1);
    let d = sys.wait_device(&id, |d| d["status"] == "Offline").await;
    assert_eq!(d["wake_error"], "Wake failed");

    let out = sys.rcli(&["devices"]).await.unwrap();
    assert!(out.contains("Wake failed"), "{out}");
}

#[tokio::test]
async fn without_a_relay_on_the_targets_subnet_the_wake_fails_at_once_with_the_reason() {
    let sys = TestSystem::start().await;
    let elsewhere = sys.start_agent_on(on_lan("elsewhere", "02:00:00:00:00:09", "10.0.0.5/24")).await;
    sys.wait_device_details(elsewhere.device_id(), |d| !d["inventory"].is_null()).await;
    let target = offline_target(&sys).await;
    let id = target.device_id().to_owned();

    let (status, body) = wake(&sys, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "no_wake_relay");
    assert!(body["error"]["message"].as_str().unwrap().contains("192.168.1.0/24"), "{body}");
    assert!(elsewhere.platform.wakes_sent().is_empty());
    sys.wait_device(&id, |d| d["status"] == "Offline").await;

    let entries = audit_wakes(&sys).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["result"]["error"], "no_wake_relay");

    let err = sys.rcli(&["wake", "target"]).await.unwrap_err();
    assert!(err.to_string().contains("192.168.1.0/24"), "{err}");
}

#[tokio::test]
async fn rcli_wake_wakes_an_offline_device_and_refuses_an_online_one() {
    let sys = TestSystem::start().await;
    let relay = sys.start_agent_on(on_lan("relay", "02:00:00:00:00:05", "192.168.1.5/24")).await;
    sys.wait_device_details(relay.device_id(), |d| !d["inventory"].is_null()).await;
    let target = offline_target(&sys).await;

    let out = sys.rcli(&["wake", "target"]).await.unwrap();
    assert!(out.contains("relay") && out.contains("aa:bb:cc:00:00:01"), "{out}");
    sys.wait_device(target.device_id(), |d| d["status"] == "Waking").await;

    let (status, body) = wake(&sys, relay.device_id()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "device_online");
}
