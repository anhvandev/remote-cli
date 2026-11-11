use std::time::{Duration, Instant};

use rcli_testkit::{Button, DisplayInfo, FakePlatform, Input, StatusCode, TestSystem};
use serde_json::{json, Value};

fn display(index: u32, x: i32, width: u32, height: u32, scale: f64) -> DisplayInfo {
    DisplayInfo { index, x, y: 0, width, height, scale, primary: index == 0 }
}

/// A Retina 2560x1440 panel (scale 2) and a 1920x1080 monitor to its right.
fn two_displays(name: &str) -> FakePlatform {
    let p = FakePlatform::new(name);
    p.set_displays(vec![display(0, 0, 2560, 1440, 2.0), display(1, 1280, 1920, 1080, 1.0)]);
    p
}

fn act(action: Value, observe: &str) -> Value {
    json!({ "actions": [action], "observe": observe })
}

#[tokio::test]
async fn input_needs_the_control_lock_but_anyone_can_observe() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice", "operator").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));
    let actions = format!("/api/v1/devices/{dev}/actions");
    let click = act(json!({"type": "click", "x": 10, "y": 10}), "none");

    let (status, body) = alice.post_json(&actions, &click).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "control_lock_required");

    let (status, lock) = alice.post(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status, StatusCode::OK, "{lock}");
    assert_eq!(lock["holder"], "alice");
    let (status, body) = bob.post(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "control_lock_held");
    assert_eq!(body["error"]["holder"], "alice");

    let (status, body) = bob.post_json(&actions, &click).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "control_lock_held");
    assert_eq!(body["error"]["holder"], "alice");
    assert!(body["error"]["message"].as_str().unwrap().contains("alice"), "{body}");
    assert!(agent.platform.inputs().is_empty());

    // Observe-only: no Actions, no lock needed.
    let (status, obs) = bob.post_json(&actions, &json!({ "actions": [] })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["images"].as_array().unwrap().len(), 1);

    let (status, body) = alice.post_json(&actions, &click).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(agent.platform.inputs().len(), 3, "move, press, release");

    let (_, status_body) = bob.get(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status_body["holder"], "alice");
    let (status, _) = bob.post(&format!("/api/v1/devices/{dev}/lock/release")).await;
    assert_eq!(status, StatusCode::CONFLICT, "only the holder releases");
    let (status, _) = alice.post(&format!("/api/v1/devices/{dev}/lock/release")).await;
    assert_eq!(status, StatusCode::OK);
    let (_, status_body) = bob.get(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!(status_body["holder"], Value::Null);
    let (status, lock) = bob.post(&format!("/api/v1/devices/{dev}/lock")).await;
    assert_eq!((status, lock["holder"].clone()), (StatusCode::OK, json!("bob")));
}

#[tokio::test]
async fn coordinates_in_observation_space_map_to_real_pixels_per_display() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(two_displays("dual")).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let (status, displays) = sys.api.get(&format!("/api/v1/devices/{dev}/displays")).await;
    assert_eq!(status, StatusCode::OK, "{displays}");
    assert_eq!(
        displays,
        json!([
            {"index": 0, "x": 0, "y": 0, "width": 2560, "height": 1440, "scale": 2.0, "primary": true},
            {"index": 1, "x": 1280, "y": 0, "width": 1920, "height": 1080, "scale": 1.0, "primary": false},
        ])
    );

    // Both displays are observed at 1280x720, so the same image point lands on different real pixels.
    let (status, obs) = sys.api.post_json(&actions, &act(json!({"type": "move", "display": 1, "x": 640, "y": 360}), "shot")).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["images"][0]["display"], 1, "default Observation is the display last acted on");
    assert_eq!((obs["images"][0]["width"].clone(), obs["images"][0]["height"].clone()), (json!(1280), json!(720)));
    sys.api.post_json(&actions, &act(json!({"type": "move", "display": 0, "x": 640, "y": 360}), "none")).await;
    sys.api.post_json(&actions, &act(json!({"type": "move", "display": 0, "x": 1279, "y": 719}), "none")).await;
    assert_eq!(
        agent.platform.inputs(),
        vec![
            Input::Move { display: 1, x: 960, y: 540 },
            Input::Move { display: 0, x: 1280, y: 720 },
            Input::Move { display: 0, x: 2558, y: 1438 },
        ]
    );

    let (status, body) = sys.api.post_json(&actions, &act(json!({"type": "move", "display": 0, "x": 1280, "y": 0}), "none")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_action");
    let (status, _) = sys.api.post_json(&actions, &act(json!({"type": "move", "display": 7, "x": 1, "y": 1}), "none")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, obs) = sys.api.post_json(&actions, &json!({ "actions": [], "display": "all" })).await;
    let images = obs["images"].as_array().unwrap();
    assert_eq!(images.iter().map(|i| i["display"].clone()).collect::<Vec<_>>(), [json!(0), json!(1)]);
    let (_, obs) = sys.api.post_json(&actions, &json!({ "actions": [], "display": 1 })).await;
    assert_eq!(obs["images"][0]["display"], 1);
}

/// Direct control from Live View addresses the streamed (native) pixels, so a click lands on the pixel under the
/// pointer instead of the nearest point of the 1280 px Observation grid.
#[tokio::test]
async fn native_points_address_real_pixels_for_direct_control() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(two_displays("native")).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let moved = act(json!({"type": "move", "display": 0, "x": 2559, "y": 1439, "native": true}), "none");
    let (status, body) = sys.api.post_json(&actions, &moved).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let dragged = json!({"type": "drag", "display": 0, "x": 1, "y": 3, "native": true, "to": {"display": 0, "x": 5, "y": 7, "native": true}});
    let (status, body) = sys.api.post_json(&actions, &act(dragged, "none")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let inputs = agent.platform.inputs();
    assert_eq!(inputs[0], Input::Move { display: 0, x: 2559, y: 1439 });
    assert_eq!(inputs[1], Input::Move { display: 0, x: 1, y: 3 });
    assert_eq!(inputs.iter().filter(|i| matches!(i, Input::Move { x: 5, y: 7, .. })).count(), 1, "{inputs:?}");

    let outside = act(json!({"type": "move", "display": 0, "x": 2560, "y": 0, "native": true}), "none");
    let (status, body) = sys.api.post_json(&actions, &outside).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_action");
}

#[tokio::test]
async fn every_action_kind_reaches_the_platform() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await; // one 1920x1080 display, observed at 1280x720
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let body = json!({ "observe": "none", "actions": [
        {"type": "click", "x": 2, "y": 2},
        {"type": "double_click", "x": 2, "y": 2},
        {"type": "right_click", "x": 2, "y": 2},
        {"type": "drag", "x": 0, "y": 0, "to": {"x": 4, "y": 4}},
        {"type": "scroll", "x": 2, "y": 2, "dx": 0, "dy": -3},
        {"type": "type", "text": "héllo"},
        {"type": "key", "keys": "ctrl+shift+t"},
    ]});
    let (status, obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs, json!({ "images": [] }), "observe none");

    use Input::*;
    let at = |x, y| Move { display: 0, x, y };
    let (ld, lu) = down_up(Button::Left);
    let (rd, ru) = down_up(Button::Right);
    assert_eq!(
        agent.platform.inputs(),
        vec![
            at(3, 3), ld.clone(), lu.clone(),
            at(3, 3), ld.clone(), lu.clone(), ld.clone(), lu.clone(),
            at(3, 3), rd, ru,
            at(0, 0), ld, at(6, 6), lu,
            at(3, 3), Scroll { dx: 0, dy: -3 },
            Text("héllo".into()),
            KeyDown("ctrl".into()), KeyDown("shift".into()), Key("t".into()), KeyUp("shift".into()), KeyUp("ctrl".into()),
        ]
    );
}

fn down_up(button: Button) -> (Input, Input) {
    (Input::Press { button, down: true }, Input::Press { button, down: false })
}

#[tokio::test]
async fn shot_waits_for_the_screen_to_settle_and_repeats_say_unchanged() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    // The screen keeps changing for 300 ms after the click.
    agent.platform.animate_after_input(Duration::from_millis(300));
    let started = Instant::now();
    let (status, obs) = sys.api.post_json(&actions, &act(json!({"type": "click", "x": 1, "y": 1}), "shot")).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(started.elapsed() >= Duration::from_millis(300), "returned before the screen settled");
    let image = &obs["images"][0];
    assert_eq!((image["width"].clone(), image["height"].clone(), image["unchanged"].clone()), (json!(1280), json!(720), json!(false)));
    assert_eq!(image["mime"], "image/jpeg");
    let jpeg = base64_decode(image["data"].as_str().unwrap());
    assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
    assert!(jpeg.len() < 100_000, "compressed: {} bytes", jpeg.len());

    // The shot captured the settled screen, so nothing differs now.
    let (_, obs) = sys.api.post_json(&actions, &json!({ "actions": [] })).await;
    assert_eq!(obs["images"][0]["unchanged"], true, "{obs}");
    assert_eq!(obs["images"][0]["data"], Value::Null);

    // "Unchanged" is relative to what each Controller saw.
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));
    let (_, obs) = bob.post_json(&actions, &json!({ "actions": [] })).await;
    assert_eq!(obs["images"][0]["unchanged"], false);

    // A screen that never settles still returns after about a second.
    agent.platform.animate_after_input(Duration::from_secs(60));
    let started = Instant::now();
    let (status, _) = sys.api.post_json(&actions, &act(json!({"type": "key", "keys": "a"}), "shot")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn actions_on_an_offline_device_fail_clearly() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    agent.stop();
    sys.wait_device(&dev, |d| !d["last_seen_at"].is_null()).await;
    let (status, body) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &json!({ "actions": [] })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "device_not_connected");
    let (status, body) = sys.api.post("/api/v1/devices/00000000-0000-0000-0000-000000000000/lock").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "device_not_found");
}

#[tokio::test]
async fn cli_lock_act_shot_and_displays() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(two_displays("dual")).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let out = sys.rcli(&["lock", "acquire", "dual"]).await.unwrap(); // by name
    assert!(out.contains("admin"), "{out}");
    let out = sys.rcli(&["lock", "status", dev, "--json"]).await.unwrap();
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["holder"], "admin");

    let out = sys.rcli(&["displays", dev]).await.unwrap();
    assert!(out.contains("2560x1440") && out.contains("1920x1080"), "{out}");

    let dir = std::env::temp_dir().join(format!("rcli-shot-{dev}"));
    let dir_s = dir.to_str().unwrap();
    let out = sys.rcli(&["act", dev, "click", "640", "360", "--display", "1", "--out", dir_s]).await.unwrap();
    assert!(out.contains("display 1"), "{out}");
    assert_eq!(agent.platform.inputs()[0], Input::Move { display: 1, x: 960, y: 540 });
    assert!(std::fs::read(dir.join("display-1.jpg")).unwrap().starts_with(&[0xFF, 0xD8]));

    sys.rcli(&["act", dev, "type", "hi", "--observe", "none"]).await.unwrap();
    sys.rcli(&["act", dev, "key", "ctrl+c", "--observe", "none"]).await.unwrap();
    sys.rcli(&["act", dev, "drag", "1", "1", "2", "2", "--observe", "none"]).await.unwrap();
    sys.rcli(&["act", dev, "scroll", "1", "1", "--dy", "5", "--observe", "none"]).await.unwrap();
    for kind in ["move", "double-click", "right-click"] {
        sys.rcli(&["act", dev, kind, "1", "1", "--observe", "none"]).await.unwrap();
    }
    assert!(agent.platform.inputs().contains(&Input::Text("hi".into())));

    let out = sys.rcli(&["shot", dev, "--display", "all", "--out", dir_s]).await.unwrap();
    assert!(out.contains("display 0") && out.contains("display 1"), "{out}");
    let out = sys.rcli(&["shot", dev, "--display", "all", "--out", dir_s]).await.unwrap();
    assert!(out.contains("unchanged"), "{out}");

    sys.rcli(&["lock", "release", dev]).await.unwrap();
    let out = sys.rcli(&["lock", "status", dev]).await.unwrap();
    assert!(out.contains("free"), "{out}");
    let err = sys.rcli(&["act", dev, "click", "1", "1"]).await.unwrap_err();
    assert!(err.to_string().contains("control_lock_required"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}
