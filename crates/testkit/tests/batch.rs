use std::time::{Duration, Instant};

use rcli_testkit::{Button, Input, StatusCode, TestSystem};
use serde_json::json;

#[tokio::test]
async fn a_batch_runs_in_order_with_waits_and_stops_at_the_failing_step() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let batch = json!({ "observe": "none", "actions": [
        {"type": "click", "x": 10, "y": 10},
        {"type": "wait", "ms": 300},
        {"type": "type", "text": "hi"},
    ]});
    let started = Instant::now();
    let (status, body) = sys.api.post_json(&actions, &batch).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(started.elapsed() >= Duration::from_millis(300), "the wait was skipped");
    let (down, up) = (Input::Press { button: Button::Left, down: true }, Input::Press { button: Button::Left, down: false });
    assert_eq!(agent.platform.inputs(), vec![Input::Move { display: 0, x: 15, y: 15 }, down, up, Input::Text("hi".into())]);

    let batch = json!({ "observe": "none", "actions": [
        {"type": "type", "text": "a"},
        {"type": "click", "x": 99999, "y": 1},
        {"type": "type", "text": "b"},
    ]});
    let (status, body) = sys.api.post_json(&actions, &batch).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!((body["error"]["code"].clone(), body["error"]["step"].clone()), (json!("invalid_action"), json!(1)));
    let inputs = agent.platform.inputs();
    assert!(inputs.contains(&Input::Text("a".into())) && !inputs.contains(&Input::Text("b".into())), "{inputs:?}");

    // Waits are bounded so a batch always answers before the Agent request times out.
    let (status, body) = sys.api.post_json(&actions, &json!({ "actions": [{"type": "wait", "ms": 60_000}] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_action");
}

#[tokio::test]
async fn frames_keep_only_changes_with_timestamps_up_to_the_cap() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    // A static screen that changes once per key: the before and after frames only.
    let batch = json!({ "observe": "frames", "actions": [
        {"type": "key", "keys": "a"},
        {"type": "wait", "ms": 300},
        {"type": "key", "keys": "b"},
    ]});
    let (status, obs) = sys.api.post_json(&actions, &batch).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    let frames = obs["images"].as_array().unwrap();
    assert_eq!(frames.len(), 3, "{obs}");
    let at: Vec<u64> = frames.iter().map(|f| f["at_ms"].as_u64().unwrap()).collect();
    assert!(at[0] < 100 && at[1] < at[2] && at[2] >= 300, "{at:?}");
    assert!(frames.iter().all(|f| f["display"] == 0 && f["unchanged"] == false && f["data"].is_string()));

    // The last frame is the final screen, so a shot right after says "unchanged".
    let (_, shot) = sys.api.post_json(&actions, &json!({ "actions": [] })).await;
    assert_eq!(shot["images"][0]["unchanged"], true, "{shot}");

    // A screen that never stops changing is capped, and still answers quickly.
    agent.platform.animate_after_input(Duration::from_secs(60));
    let started = Instant::now();
    let batch = json!({ "observe": "frames", "max_frames": 3, "actions": [{"type": "key", "keys": "c"}, {"type": "wait", "ms": 500}] });
    let (status, obs) = sys.api.post_json(&actions, &batch).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(started.elapsed() < Duration::from_secs(4));
    let frames = obs["images"].as_array().unwrap();
    assert_eq!(frames.len(), 3, "{obs}");
    assert!(frames[2]["at_ms"].as_u64().unwrap() >= 1_000, "the newest frame replaces the last one: {:?}", frames[2]["at_ms"]);

    let (status, body) = sys.api.post_json(&actions, &json!({ "observe": "frames", "max_frames": 0 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn cli_runs_a_batch_from_json() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.rcli(&["lock", "acquire", dev]).await.unwrap();
    let dir = std::env::temp_dir().join(format!("rcli-batch-{dev}"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("batch.json");

    // A bare list of Actions.
    std::fs::write(&file, r#"[{"type": "type", "text": "hi"}, {"type": "wait", "ms": 50}, {"type": "key", "keys": "enter"}]"#).unwrap();
    let out = sys.rcli(&["batch", dev, file.to_str().unwrap(), "--observe", "none"]).await.unwrap();
    assert_eq!(out, "ok\n");
    assert_eq!(agent.platform.inputs(), vec![Input::Text("hi".into()), Input::Key("enter".into())]);

    // Frames are saved one file per frame.
    let out = sys.rcli(&["batch", dev, file.to_str().unwrap(), "--observe", "frames", "--out", dir.to_str().unwrap()]).await.unwrap();
    let saved: Vec<&str> = out.lines().collect();
    assert_eq!(saved.len(), 3, "{out}");
    for line in saved {
        let path = line.rsplit(' ').next().unwrap();
        assert!(std::fs::read(path).unwrap().starts_with(&[0xFF, 0xD8]), "{line}");
    }

    // A failing step is named.
    std::fs::write(&file, r#"[{"type": "key", "keys": "a"}, {"type": "click", "x": 99999, "y": 0}]"#).unwrap();
    let err = sys.rcli(&["batch", dev, file.to_str().unwrap()]).await.unwrap_err().to_string();
    assert!(err.contains("invalid_action") && err.contains("step 1"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}
