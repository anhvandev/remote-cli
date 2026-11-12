//! Ticket 12: a Device without a display keeps metrics, exec, shell, power and Wake, and refuses shot, act,
//! batch and Live View with a typed `no_display` error.

use rcli_testkit::{
    platform::{NetworkInterface, PowerAction},
    FakePlatform, StatusCode, TestSystem,
};
use serde_json::json;

/// A platform that reports no displays, as a headless server does.
fn headless(hostname: &str) -> FakePlatform {
    let p = FakePlatform::new(hostname);
    p.set_displays(vec![]);
    p
}

#[tokio::test]
async fn screen_actions_are_refused_with_no_display() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(headless("headless")).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");
    let refused = |status: StatusCode, body: &serde_json::Value| {
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "no_display", "{body}");
        assert_eq!(body["error"]["message"], "Device has no display", "{body}");
    };

    // shot: observe only.
    let (status, body) = sys.api.post_json(&actions, &json!({ "actions": [] })).await;
    refused(status, &body);

    // act and batch: the Control Lock is taken first, so they reach the Agent.
    assert_eq!(sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await.0, StatusCode::OK);
    let (status, body) = sys.api.post_json(&actions, &json!({ "actions": [{ "type": "click", "x": 1, "y": 1 }] })).await;
    refused(status, &body);
    let batch = json!({ "actions": [{ "type": "move", "x": 1, "y": 1 }, { "type": "wait", "ms": 1 }], "observe": "none" });
    let (status, body) = sys.api.post_json(&actions, &batch).await;
    refused(status, &body);
    assert!(agent.platform.inputs().is_empty(), "nothing reached the OS");

    let err = sys.rcli(&["shot", &dev]).await.unwrap_err();
    let err = format!("{err:#}");
    assert!(err.contains("no_display") && err.contains("Device has no display"), "{err}");
}

#[tokio::test]
async fn live_view_is_refused_with_no_display() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(headless("headless")).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let live = format!("/api/v1/devices/{dev}/live");

    let (status, body) = sys.api.post_json(&live, &json!({ "sdp": "v=0" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "no_display", "{body}");
    assert_eq!(body["error"]["message"], "Device has no display", "{body}");
}

#[tokio::test]
async fn metrics_exec_shell_power_and_wake_still_work() {
    let sys = TestSystem::start().await;
    // headless has a NIC so a Wake Relay on its subnet can be found.
    let platform = headless("headless");
    platform.update(|s| {
        s.inventory.network_interfaces =
            vec![NetworkInterface { name: "eth0".into(), mac: "02:00:00:00:00:31".into(), ips: vec!["192.168.1.31/24".into()] }]
    });
    let mut agent = sys.start_agent_on(platform).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;

    // Metrics reach the Server with the Device Online.
    let details = sys.wait_device_details(&dev, |d| d["resources"]["cpu_percent"].is_number()).await;
    assert_eq!(details["resources"]["cpu_percent"], json!(12.5));

    // exec
    let (status, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &json!({ "command": "echo hi" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["stdout"], "hi\n");

    // shell: the stream stays open until the shell exits, so assert acceptance and drop it.
    let token = sys.create_user("alice").await;
    let base = format!("http://{}", sys.server.http_addr);
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/v1/devices/{dev}/shell"))
        .bearer_auth(&token)
        .json(&json!({ "cols": 80, "rows": 24 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    drop(resp);

    // power
    let (status, body) = sys.api.post_json(&format!("/api/v1/devices/{dev}/power"), &json!({ "action": "lock" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(agent.platform.power_actions(), vec![PowerAction::Lock]);

    // Wake: go Offline, then a relay on the same subnet sends the packet.
    let relay = sys
        .start_agent_on({
            let p = FakePlatform::new("relay");
            p.update(|s| {
                s.inventory.network_interfaces =
                    vec![NetworkInterface { name: "eth0".into(), mac: "02:00:00:00:00:05".into(), ips: vec!["192.168.1.5/24".into()] }]
            });
            p
        })
        .await;
    sys.wait_device_details(relay.device_id(), |d| !d["inventory"].is_null()).await;
    sys.wait_device_details(&dev, |d| !d["inventory"].is_null()).await;
    agent.stop();
    sys.wait_device(&dev, |d| !d["last_seen_at"].is_null()).await;
    sys.advance(31);
    sys.wait_device(&dev, |d| d["status"] == "Offline").await;
    let (status, body) = sys.api.post(&format!("/api/v1/devices/{dev}/wake")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["relay"]["name"], "relay");
    assert_eq!(relay.platform.wakes_sent(), vec![[0x02, 0x00, 0x00, 0x00, 0x00, 0x31]]);
}
