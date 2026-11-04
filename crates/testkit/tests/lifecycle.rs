use std::time::Duration;

use rcli_testkit::{StatusCode, TestSystem};

#[tokio::test]
async fn revoked_device_is_disconnected_and_cannot_reconnect() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let id = agent.device_id().to_owned();
    sys.wait_device(&id, |d| d["status"] == "Online" && d["last_seen_at"].is_null()).await;

    let operator = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, _) = operator.post(&format!("/api/v1/devices/{id}/revoke")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = sys.api.post("/api/v1/devices/lab-pc/revoke").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["revoked_at"].is_string(), "{body}");

    let dropped = sys.wait_device(&id, |d| !d["last_seen_at"].is_null()).await;
    // The Agent retries after 1 s; it must stay disconnected.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    sys.wait_device(&id, |d| d["last_seen_at"] == dropped["last_seen_at"]).await;
    sys.advance(30);
    sys.wait_device(&id, |d| d["status"] == "Offline").await;
}

#[tokio::test]
async fn admin_renames_and_deletes_devices_with_rcli() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let id = agent.device_id().to_owned();
    sys.wait_device(&id, |d| d["status"] == "Online").await;

    let out = sys.rcli(&["device", "rename", "lab-pc", "reception"]).await.unwrap();
    assert!(out.contains("reception"), "{out}");
    sys.wait_device(&id, |d| d["name"] == "reception").await;
    let (status, body) = sys.api.patch_json(&format!("/api/v1/devices/{id}"), &serde_json::json!({"name": " "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let operator = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, _) = operator.delete(&format!("/api/v1/devices/{id}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    sys.rcli(&["device", "delete", "reception"]).await.unwrap();
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices, serde_json::json!([]));
    // Its connection is gone and its certificate no longer gets in.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices, serde_json::json!([]));
    let (status, _) = sys.api.get(&format!("/api/v1/devices/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn cpus(history: &serde_json::Value) -> Vec<f64> {
    history["samples"].as_array().unwrap().iter().filter_map(|s| s["resources"]["cpu_percent"].as_f64()).collect()
}

#[tokio::test]
async fn reinstalled_machine_is_a_new_device_until_an_admin_merges_the_old_history_into_it() {
    let sys = TestSystem::start().await;
    sys.advance(10); // off the minute boundary
    let mut old = sys.start_agent("lab-pc").await;
    let old_id = old.device_id().to_owned();
    old.platform.update(|s| s.resources.cpu_percent = Some(10.0));
    sys.wait_history(&old_id, "resolution=5s", |h| cpus(h).contains(&10.0)).await;
    let (status, body) = sys.api.post(&format!("/api/v1/devices/{old_id}/lock")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let click = serde_json::json!({"actions": [{"type": "click", "display": 0, "x": 1, "y": 1}], "observe": "none"});
    let (status, body) = sys.api.post_json(&format!("/api/v1/devices/{old_id}/actions"), &click).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    old.stop();
    sys.wait_device(&old_id, |d| !d["last_seen_at"].is_null()).await;

    // Same machine, reinstalled: enrolls again and is NOT matched to the old Device.
    sys.advance(60);
    old.platform.update(|s| s.resources.cpu_percent = Some(20.0));
    let new = sys.start_agent_on(old.platform.clone()).await;
    let new_id = new.device_id().to_owned();
    assert_ne!(new_id, old_id);
    sys.wait_history(&new_id, "resolution=5s", |h| cpus(h).contains(&20.0)).await;
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices.as_array().unwrap().iter().filter(|d| d["name"] == "lab-pc").count(), 2, "{devices}");
    sys.advance(60);
    let range = "from=2026-01-01T00:00:00Z";
    assert_eq!(cpus(&sys.history(&new_id, &format!("{range}&resolution=1m")).await), vec![20.0]);

    let (status, _) = sys.api.post_json(&format!("/api/v1/devices/{new_id}/merge"), &serde_json::json!({"from": new_id})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let out = sys.rcli(&["device", "merge", &old_id, "--into", &new_id]).await.unwrap();
    assert!(out.contains(&new_id), "{out}");

    assert_eq!(cpus(&sys.history(&new_id, &format!("{range}&resolution=1m")).await), vec![10.0, 20.0]);
    let raw = cpus(&sys.history(&new_id, &format!("{range}&resolution=5s")).await);
    assert!(raw.contains(&10.0) && raw.contains(&20.0), "{raw:?}");
    // Audit entries about the old Device now point at the new one (followed by the merge itself).
    let (_, audit) = sys.api.get(&format!("/api/v1/audit?device={new_id}")).await;
    assert_eq!(audit.as_array().map(Vec::len), Some(2), "{audit}");
    assert_eq!((&audit[0]["action"], &audit[0]["device"]), (&"act".into(), &"lab-pc".into()), "{audit}");
    assert_eq!(audit[1]["action"], "device.merge", "{audit}");
    // The old Device is gone; the new one keeps its identity and connection.
    let (status, _) = sys.api.get(&format!("/api/v1/devices/{old_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    sys.wait_device(&new_id, |d| d["status"] == "Online").await;
}
