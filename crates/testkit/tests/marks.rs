use rcli_testkit::rmcp::model::CallToolRequestParams;
use rcli_testkit::{Button, FakePlatform, Input, SessionState, StatusCode, TestSystem, UiElement};
use serde_json::json;

fn element(role: &str, text: &str, x: u32, y: u32, width: u32, height: u32) -> UiElement {
    UiElement { role: role.into(), text: text.into(), x, y, width, height, ..Default::default() }
}

const LONG_NOTE: &str = "Ship the release notes to the whole team before Friday";

/// A "Save changes" dialog (element 0) holding a ticked checkbox, a filled field, a password field whose adapter
/// wrongly reports its value, a disabled and a focused button, and a field with a long value.
fn save_dialog() -> FakePlatform {
    let p = FakePlatform::new("lab-pc");
    let states = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let value = |v: &str| Some(v.to_owned());
    p.update(|s| {
        s.accessibility = Some(vec![
            element("dialog", "Save changes", 300, 300, 900, 450),
            UiElement { states: states(&["checked"]), parent: Some(0), ..element("checkbox", "Remember me", 330, 390, 300, 30) },
            UiElement { value: value("An"), parent: Some(0), ..element("field", "Name", 330, 450, 600, 60) },
            UiElement { value: value("hunter2"), password: true, parent: Some(0), ..element("field", "Password", 330, 540, 600, 60) },
            UiElement { states: states(&["disabled"]), parent: Some(0), ..element("button", "Save", 330, 630, 150, 60) },
            UiElement { states: states(&["focused"]), parent: Some(0), ..element("button", "Cancel", 630, 630, 150, 60) },
            UiElement { value: value(LONG_NOTE), parent: Some(0), ..element("field", "Notes", 930, 450, 240, 60) },
        ])
    });
    p
}

/// The Mark table of `save_dialog`, in image space (real / 1.5).
fn save_dialog_marks() -> serde_json::Value {
    json!([
        {"number": 1, "text": "Save changes", "role": "dialog", "display": 0, "x": 200, "y": 200, "width": 600, "height": 300},
        {"number": 2, "text": "Remember me", "role": "checkbox", "display": 0, "x": 220, "y": 260, "width": 200, "height": 20, "states": ["checked"], "parent": 1},
        {"number": 3, "text": "Name", "role": "field", "display": 0, "x": 220, "y": 300, "width": 400, "height": 40, "value": "An", "parent": 1},
        {"number": 4, "text": "Password", "role": "field", "display": 0, "x": 220, "y": 360, "width": 400, "height": 40, "parent": 1},
        {"number": 5, "text": "Save", "role": "button", "display": 0, "x": 220, "y": 420, "width": 100, "height": 40, "states": ["disabled"], "parent": 1},
        {"number": 6, "text": "Cancel", "role": "button", "display": 0, "x": 420, "y": 420, "width": 100, "height": 40, "states": ["focused"], "parent": 1},
        {"number": 7, "text": "Notes", "role": "field", "display": 0, "x": 620, "y": 300, "width": 160, "height": 40, "value": LONG_NOTE, "parent": 1},
    ])
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
async fn marks_carry_their_states_value_and_container_and_a_container_is_a_mark() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(save_dialog()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let (status, obs) = sys.api.post_json(&actions, &json!({ "actions": [], "marks": true })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["marks"], save_dialog_marks(), "empty fields omitted, no value on the password field");

    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let body = json!({ "observe": "none", "actions": [{"type": "move", "text": "save changes"}] });
    let (status, body) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The dialog's centre (500,350) in real pixels.
    assert_eq!(agent.platform.inputs(), [Input::Move { display: 0, x: 750, y: 525 }]);
}

#[tokio::test]
async fn mcp_observe_carries_mark_states_values_and_containers_and_the_schemas_describe_them() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(save_dialog()).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let mcp = sys.mcp_as(&sys.admin_token).await;

    for tool in mcp.list_all_tools().await.unwrap().into_iter().filter(|t| ["observe", "act", "batch"].contains(&t.name.as_ref())) {
        let marks = tool.input_schema["properties"]["marks"]["description"].as_str().unwrap_or_default().to_owned();
        for word in ["states", "disabled", "checked", "value", "password", "parent", "dialog"] {
            assert!(marks.contains(word), "{}: {word} missing in {marks}", tool.name);
        }
    }
    let params = CallToolRequestParams::new("observe").with_arguments(json!({ "device": "lab-pc", "marks": true }).as_object().unwrap().clone());
    let r = mcp.call_tool(params).await.unwrap();
    assert_eq!(r.structured_content.unwrap()["marks"], save_dialog_marks());
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
    assert!(out.lines().any(|l| l.split_whitespace().eq(["2", "0", "button", "200,400", "100x40", "OK"])), "{out}");
    let observation = out.lines().find_map(|l| l.strip_prefix("observation ")).unwrap().to_owned();

    sys.rcli(&["act", dev, "click", "--mark", "3", "--observation", &observation, "--observe", "none"]).await.unwrap();
    sys.rcli(&["act", dev, "click", "--text", "OK", "--observe", "none"]).await.unwrap();
    let moves: Vec<Input> = agent.platform.inputs().into_iter().filter(|i| matches!(i, Input::Move { .. })).collect();
    assert_eq!(moves, [Input::Move { display: 0, x: 675, y: 630 }, Input::Move { display: 0, x: 375, y: 630 }]);
    let err = sys.rcli(&["act", dev, "click", "--mark", "3"]).await.unwrap_err();
    assert!(err.to_string().contains("--observation"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn cli_mark_table_shows_states_values_and_containers_and_json_the_full_value() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent_on(save_dialog()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let dir = std::env::temp_dir().join(format!("rcli-marks-{dev}"));

    let out = sys.rcli(&["shot", dev, "--marks", "--out", dir.to_str().unwrap()]).await.unwrap();
    let row = |n: &str| out.lines().find(|l| l.split_whitespace().next() == Some(n)).unwrap_or_else(|| panic!("no row {n}: {out}"));
    let words = |l: &str| l.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(words(row("MARK")), ["MARK", "DISPLAY", "ROLE", "BOX", "PARENT", "STATES", "VALUE", "TEXT"]);
    assert_eq!(words(row("1")), ["1", "0", "dialog", "200,200", "600x300", "Save", "changes"]);
    assert_eq!(words(row("2")), ["2", "0", "checkbox", "220,260", "200x20", "1", "checked", "Remember", "me"]);
    assert_eq!(words(row("3")), ["3", "0", "field", "220,300", "400x40", "1", "An", "Name"]);
    assert_eq!(words(row("4")), ["4", "0", "field", "220,360", "400x40", "1", "Password"]);
    assert!(row("7").contains("Ship the release no…  Notes"), "long value shortened: {out}");

    let json = sys.rcli(&["shot", dev, "--marks", "--json"]).await.unwrap();
    let obs: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(obs["marks"][6]["value"], LONG_NOTE);
    let _ = std::fs::remove_dir_all(dir);
}
