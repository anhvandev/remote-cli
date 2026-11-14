use rcli_testkit::{Button, FakePlatform, Input, SessionState, StatusCode, TestSystem, UiElement};
use serde_json::json;

fn element(role: &str, text: &str, x: u32, y: u32, width: u32, height: u32) -> UiElement {
    UiElement { role: role.into(), text: text.into(), x, y, width, height }
}

/// One 1920x1080 display (observed at 1280x720, so real = image * 1.5) with a small login form in its
/// accessibility tree.
fn login_form() -> FakePlatform {
    let p = FakePlatform::new("lab-pc");
    p.update(|s| {
        s.accessibility = Some(vec![
            element("text_field", "Name", 300, 300, 600, 60),
            element("button", "OK", 300, 600, 150, 60),
            element("button", "Cancel", 600, 600, 150, 60),
        ])
    });
    p
}

#[tokio::test]
async fn observation_carries_an_annotated_image_and_the_mark_table_from_the_accessibility_tree() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let (_, plain) = sys.api.post_json(&actions, &json!({ "actions": [] })).await;
    assert_eq!(plain.get("marks"), None, "no Marks unless asked for");
    let (status, obs) = sys.api.post_json(&actions, &json!({ "actions": [], "marks": true })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["images"][0]["unchanged"], false, "the annotated image differs from the plain one");
    assert!(obs["observation"].as_u64().is_some(), "{obs}");
    assert_eq!(
        obs["marks"],
        json!([
            {"number": 1, "text": "Name", "role": "text_field", "display": 0, "x": 200, "y": 200, "width": 400, "height": 40},
            {"number": 2, "text": "OK", "role": "button", "display": 0, "x": 200, "y": 400, "width": 100, "height": 40},
            {"number": 3, "text": "Cancel", "role": "button", "display": 0, "x": 400, "y": 400, "width": 100, "height": 40},
        ])
    );
}

#[tokio::test]
async fn without_an_accessibility_tree_marks_come_from_ocr() {
    let p = FakePlatform::new("lab-pc"); // no tree, like a lock screen
    p.set_session_state(SessionState::Locked);
    p.update(|s| s.screen_text = vec![element("text", "Password", 900, 540, 300, 30)]);
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(p).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let (status, obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &json!({ "actions": [], "marks": true })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(
        obs["marks"],
        json!([{"number": 1, "text": "Password", "role": "text", "display": 0, "x": 600, "y": 360, "width": 200, "height": 20}])
    );

    // Waiting for text works on OCR Marks too, e.g. for the desktop after typing the password.
    let p = agent.platform.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        p.update(|s| s.screen_text = vec![element("text", "Welcome", 900, 540, 300, 30)]);
    });
    let body = json!({ "actions": [], "marks": true, "until": {"appears": {"text": "welcome"}} });
    let (status, obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["marks"][0]["text"], "Welcome", "{obs}");
}

#[tokio::test]
async fn actions_target_marks_by_number_or_text() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");
    let (_, obs) = sys.api.post_json(&actions, &json!({ "actions": [], "marks": true })).await;
    let observation = obs["observation"].clone();

    let body = json!({ "observe": "none", "actions": [
        {"type": "click", "mark": 3, "observation": observation},
        {"type": "move", "text": "ok"},
        {"type": "type", "text": "alice", "into": {"text": "Name"}},
    ]});
    let (status, body) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (down, up) = (Input::Press { button: Button::Left, down: true }, Input::Press { button: Button::Left, down: false });
    // Mark centres in image space, mapped to real pixels: Cancel (450,420), OK (250,420), Name (400,220).
    assert_eq!(
        agent.platform.inputs(),
        vec![
            Input::Move { display: 0, x: 675, y: 630 },
            down.clone(),
            up.clone(),
            Input::Move { display: 0, x: 375, y: 630 },
            Input::Move { display: 0, x: 600, y: 330 },
            down,
            up,
            Input::Text("alice".into()),
        ]
    );
}

#[tokio::test]
async fn stale_marks_and_ambiguous_or_missing_text_fail_clearly() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");
    let click = |target: serde_json::Value| {
        let mut a = json!({"type": "click"});
        a.as_object_mut().unwrap().extend(target.as_object().unwrap().clone());
        json!({ "observe": "none", "actions": [a] })
    };

    let (status, body) = sys.api.post_json(&actions, &click(json!({"mark": 1, "observation": 1}))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("stale_mark")), "{body}");

    let (_, old) = sys.api.post_json(&actions, &json!({ "actions": [], "marks": true })).await;
    let (_, new) = sys.api.post_json(&actions, &json!({ "actions": [], "marks": true })).await;
    let (status, body) = sys.api.post_json(&actions, &click(json!({"mark": 2, "observation": old["observation"]}))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("stale_mark")), "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("observe again"), "{body}");

    let (status, body) = sys.api.post_json(&actions, &click(json!({"mark": 9, "observation": new["observation"]}))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("invalid_action")), "{body}");

    // "n" is part of "Name" and "Cancel".
    let (status, body) = sys.api.post_json(&actions, &click(json!({"text": "n"}))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("ambiguous_text")), "{body}");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("Mark 1") && message.contains("Mark 3"), "{message}");

    let (status, body) = sys.api.post_json(&actions, &click(json!({"text": "Delete"}))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("text_not_found")), "{body}");
    assert!(agent.platform.inputs().is_empty(), "nothing was clicked");
}

#[tokio::test]
async fn cli_prints_the_mark_table_and_targets_marks() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.rcli(&["lock", "acquire", dev]).await.unwrap();
    let dir = std::env::temp_dir().join(format!("rcli-marks-{dev}"));
    let dir_s = dir.to_str().unwrap();

    let out = sys.rcli(&["shot", dev, "--marks", "--out", dir_s]).await.unwrap();
    assert!(out.contains("2     0        button        200,400 100x40       OK"), "{out}");
    let observation = out.lines().find_map(|l| l.strip_prefix("observation ")).unwrap().to_owned();
    // A Wait Condition on `shot`: met at once, or ("OK" never disappears) timed out.
    sys.rcli(&["shot", dev, "--until-appears", "ok", "--role", "button", "--out", dir_s]).await.unwrap();
    let err = sys.rcli(&["shot", dev, "--until-disappears", "OK", "--timeout-ms", "300", "--out", dir_s]).await.unwrap_err();
    assert!(err.to_string().contains("wait_timeout"), "{err}");

    sys.rcli(&["act", dev, "click", "--mark", "3", "--observation", &observation, "--observe", "none"]).await.unwrap();
    sys.rcli(&["act", dev, "click", "--text", "OK", "--observe", "none"]).await.unwrap();
    let moves: Vec<Input> = agent.platform.inputs().into_iter().filter(|i| matches!(i, Input::Move { .. })).collect();
    assert_eq!(moves, [Input::Move { display: 0, x: 675, y: 630 }, Input::Move { display: 0, x: 375, y: 630 }]);
    let err = sys.rcli(&["act", dev, "click", "--mark", "3"]).await.unwrap_err();
    assert!(err.to_string().contains("--observation"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Changes FakePlatform's tree after `ms`, while a request is waiting.
fn later(p: &FakePlatform, ms: u64, edit: impl FnOnce(&mut Vec<UiElement>) + Send + 'static) {
    let p = p.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        p.update(|s| edit(s.accessibility.as_mut().unwrap()));
    });
}

fn texts(obs: &serde_json::Value) -> Vec<serde_json::Value> {
    obs["marks"].as_array().unwrap().iter().map(|m| m["text"].clone()).collect()
}

#[tokio::test]
async fn observe_waits_until_text_appears_or_disappears_without_the_control_lock() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    later(&agent.platform, 500, |tree| tree.push(element("text", "Saved", 300, 900, 150, 30)));
    let body = json!({ "actions": [], "marks": true, "until": {"appears": {"text": "saved"}} });
    let (status, obs) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(texts(&obs).contains(&json!("Saved")), "{obs}");

    later(&agent.platform, 500, |tree| tree.retain(|e| e.text != "Saved"));
    let body = json!({ "actions": [], "marks": true, "until": {"disappears": {"text": "Saved", "role": "text"}} });
    let (status, obs) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(!texts(&obs).contains(&json!("Saved")), "{obs}");
}

#[tokio::test]
async fn a_wait_condition_never_met_times_out_with_the_observation() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(login_form()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let started = std::time::Instant::now();
    let body = json!({ "actions": [], "marks": true, "until": {"appears": {"text": "Saved"}, "timeout_ms": 1000} });
    let (status, body) = sys.api.post_json(&actions, &body).await;
    let waited = started.elapsed();
    assert_eq!(body["error"]["code"], "wait_timeout", "{body}");
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(waited >= std::time::Duration::from_millis(1000) && waited < std::time::Duration::from_millis(4000), "waited {waited:?}");
    assert_eq!(texts(&body["error"]), [json!("Name"), json!("OK"), json!("Cancel")], "{body}");
    assert!(body["error"]["images"][0]["data"].is_string(), "{body}");
}
