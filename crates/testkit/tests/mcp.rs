//! `rcli mcp`: an AI agent drives a Device over MCP (stdio transport, here an in-memory pipe).

use rcli_testkit::rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rcli_testkit::{eventually, Clock, FakePlatform, Input, TestSystem, UiElement};
use serde_json::{json, Value};

type Client = rcli_testkit::rmcp::service::RunningService<rcli_testkit::rmcp::RoleClient, ()>;

async fn call(mcp: &Client, tool: &str, args: Value) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(args.as_object().unwrap().clone());
    mcp.call_tool(params).await.unwrap()
}

/// The structured JSON of a successful call.
async fn ok(mcp: &Client, tool: &str, args: Value) -> (Value, CallToolResult) {
    let r = call(mcp, tool, args).await;
    assert_ne!(r.is_error, Some(true), "{tool}: {r:?}");
    (r.structured_content.clone().unwrap_or_else(|| panic!("{tool}: no structured content: {r:?}")), r)
}

fn images(r: &CallToolResult) -> Vec<(String, String)> {
    r.content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Image(i) => Some((i.mime_type.clone(), i.data.clone())),
            _ => None,
        })
        .collect()
}

fn text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_agent_lists_devices_takes_the_lock_acts_and_observes_with_marks() {
    let sys = TestSystem::start().await;
    let p = FakePlatform::new("lab-pc");
    p.update(|s| s.accessibility = Some(vec![UiElement { role: "button".into(), text: "OK".into(), x: 300, y: 600, width: 150, height: 60 }]));
    let agent = sys.start_agent_on(p.clone()).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let mcp = sys.mcp_as(&sys.admin_token).await;

    let all = mcp.list_all_tools().await.unwrap();
    let observe = all.iter().find(|t| t.name == "observe").unwrap();
    assert!(observe.input_schema["properties"]["until"]["description"].as_str().unwrap().contains("appears"), "{observe:?}");
    let tools: Vec<String> = all.into_iter().map(|t| t.name.into_owned()).collect();
    for t in ["devices", "device", "lock_acquire", "lock_release", "lock_status", "act", "batch", "observe", "exec", "wake", "power"] {
        assert!(tools.contains(&t.to_owned()), "missing tool {t}: {tools:?}");
    }

    let (devices, _) = ok(&mcp, "devices", json!({})).await;
    assert_eq!(devices["devices"][0]["name"], "lab-pc", "{devices}");
    let (details, _) = ok(&mcp, "device", json!({ "device": "lab-pc" })).await;
    assert_eq!(details["device_id"], dev);

    let (lock, _) = ok(&mcp, "lock_acquire", json!({ "device": "lab-pc" })).await;
    assert_eq!(lock["holder"], "admin");

    // act -> the Observation comes back as an inline JPEG.
    let (obs, r) = ok(&mcp, "act", json!({ "device": "lab-pc", "action": {"type": "click", "x": 10, "y": 10} })).await;
    let imgs = images(&r);
    assert_eq!(imgs.len(), 1, "{r:?}");
    assert_eq!(imgs[0].0, "image/jpeg");
    assert!(!imgs[0].1.is_empty());
    assert_eq!(obs["images"][0]["data"], Value::Null, "image bytes go in the image block, not the JSON: {obs}");
    assert!(p.inputs().contains(&Input::Move { display: 0, x: 15, y: 15 }), "{:?}", p.inputs());

    // observe with Marks -> annotated image + the Mark table as data, then click a Mark by number.
    let (obs, r) = ok(&mcp, "observe", json!({ "device": "lab-pc", "marks": true })).await;
    assert_eq!(images(&r).len(), 1);
    assert_eq!(obs["marks"][0]["text"], "OK", "{obs}");
    // observe with a Wait Condition: "OK" never disappears, so the wait times out.
    let r = call(&mcp, "observe", json!({ "device": "lab-pc", "until": {"disappears": {"text": "OK"}, "timeout_ms": 300} })).await;
    assert!(r.is_error == Some(true) && text(&r).contains("wait_timeout"), "{r:?}");
    let mark = json!({ "type": "click", "mark": 1, "observation": obs["observation"] });
    let (_, r) = ok(&mcp, "batch", json!({ "device": "lab-pc", "actions": [mark, {"type": "wait", "ms": 10}], "observe": "none" })).await;
    assert!(images(&r).is_empty());

    let (out, _) = ok(&mcp, "exec", json!({ "device": "lab-pc", "command": "echo hi" })).await;
    assert_eq!((out["exit_code"].clone(), out["stdout"].clone()), (json!(0), json!("hi\n")));

    let (lock, _) = ok(&mcp, "lock_release", json!({ "device": "lab-pc" })).await;
    assert_eq!(lock["holder"], Value::Null);
    let (lock, _) = ok(&mcp, "lock_status", json!({ "device": "lab-pc" })).await;
    assert_eq!(lock["holder"], Value::Null);
}

#[tokio::test]
async fn the_instructions_say_to_type_the_os_password_as_a_secret_type_action() {
    let sys = TestSystem::start().await;
    let mcp = sys.mcp_as(&sys.admin_token).await;
    let instructions = mcp.peer_info().and_then(|i| i.instructions.clone()).unwrap_or_default();
    assert!(
        instructions.contains("lock or login screen") && instructions.contains("\"secret\":true") && instructions.contains("does not store"),
        "{instructions}"
    );
}

#[tokio::test]
async fn a_held_control_lock_is_a_readable_tool_error() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let bob = sys.create_user("bob").await;
    sys.rcli_as(&bob, &["lock", "acquire", "lab-pc"]).await.unwrap();
    let mcp = sys.mcp_as(&sys.admin_token).await;

    for (tool, args) in [
        ("lock_acquire", json!({ "device": "lab-pc" })),
        ("act", json!({ "device": "lab-pc", "action": {"type": "key", "keys": "enter"} })),
    ] {
        let r = call(&mcp, tool, args).await;
        assert_eq!(r.is_error, Some(true), "{tool}: {r:?}");
        let t = text(&r);
        assert!(t.contains("control_lock_held") && t.contains("bob"), "{tool}: {t}");
    }
    let r = call(&mcp, "device", json!({ "device": "nope" })).await;
    assert!(text(&r).contains("device_not_found"), "{r:?}");
}

#[tokio::test]
async fn the_lock_ends_60_seconds_after_the_mcp_server_goes_away() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let mcp = sys.mcp_as(&sys.admin_token).await;
    ok(&mcp, "lock_acquire", json!({ "device": "lab-pc" })).await;

    // While the MCP server runs it counts as connected: no 60 s grace kicks in (a closed watch is noticed
    // at the Server's next 5 s status write, so wait past one).
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    sys.advance(61);
    let (_, l) = sys.api.get("/api/v1/devices/lab-pc/lock").await;
    assert_eq!(l["holder"], "admin", "{l}");

    // Closing the MCP session (agent crashed) drops the lock watch: 60 s grace instead of the 10 min idle timer.
    mcp.cancel().await.unwrap();
    let time = |v: &Value| v.as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    eventually(|| async {
        let (_, l) = sys.api.get("/api/v1/devices/lab-pc/lock").await;
        (time(&l["expires_at"]) - sys.clock.now() <= chrono::Duration::seconds(60)).then_some(())
    })
    .await;
    sys.advance(60);
    let (_, l) = sys.api.get("/api/v1/devices/lab-pc/lock").await;
    assert_eq!(l["holder"], Value::Null);
}

#[tokio::test]
async fn an_agent_locks_the_screen_and_restart_is_refused_while_someone_else_holds_the_lock() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let bob = sys.api.with_token(Some(&sys.create_user("bob").await));
    bob.post("/api/v1/devices/lab-pc/lock").await;
    let mcp = sys.mcp_as(&sys.create_user("alice").await).await;

    let (r, _) = ok(&mcp, "power", json!({ "device": "lab-pc", "action": "lock" })).await;
    assert_eq!(r, json!({ "action": "lock" }));
    let refused = call(&mcp, "power", json!({ "device": "lab-pc", "action": "restart" })).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(text(&refused).contains("control_lock_held") && text(&refused).contains("bob"), "{}", text(&refused));
    assert_eq!(agent.platform.power_actions(), vec![rcli_testkit::platform::PowerAction::Lock]);
}

#[tokio::test]
async fn releasing_by_id_a_lock_taken_by_name_closes_its_watch() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let mcp = sys.mcp_as(&sys.admin_token).await;
    ok(&mcp, "lock_acquire", json!({ "device": "lab-pc" })).await;
    ok(&mcp, "lock_release", json!({ "device": dev })).await;

    // The same user takes the lock again elsewhere (e.g. the Dashboard) and closes that watch: with no
    // leftover MCP watch they count as disconnected, so the 60 s grace applies.
    sys.api.post("/api/v1/devices/lab-pc/lock").await;
    drop(sys.api.watch("/api/v1/devices/lab-pc/lock/watch").await);
    let time = |v: &Value| v.as_str().unwrap().parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    eventually(|| async {
        let (_, l) = sys.api.get("/api/v1/devices/lab-pc/lock").await;
        (time(&l["expires_at"]) - sys.clock.now() <= chrono::Duration::seconds(60)).then_some(())
    })
    .await;
}

#[tokio::test]
async fn mcp_reads_the_saved_config_with_no_env_vars() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    // A saved User session (rcli login) is enough for a MCP client started from the config.
    let dir = std::path::Path::new("/tmp").join(format!("rcli05mcp-{}-login", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    sys.controller_config(&dir, None).await;
    let secret = sys.create_dashboard_user("mcp-user").await;
    sys.rcli_in(&dir, &["login", "mcp-user", "--password", TestSystem::PASSWORD, "--totp", &sys.totp(&secret)])
        .await
        .unwrap();
    let mcp = sys.mcp_in(&dir).await;
    let (devices, _) = ok(&mcp, "devices", json!({})).await;
    assert!(devices["devices"].as_array().is_some_and(|d| !d.is_empty()), "{devices}");
    mcp.cancel().await.unwrap();

    // No session: a remembered Device Password makes the connection, as `rcli connect` saved it.
    let dir = std::path::Path::new("/tmp").join(format!("rcli05mcp-{}-connect", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    sys.controller_config(&dir, Some((agent.device_id(), &agent.device_password().unwrap()))).await;
    let mcp = sys.mcp_in(&dir).await;
    let (details, _) = ok(&mcp, "device", json!({ "device": agent.device_id() })).await;
    assert_eq!(details["device_id"], agent.device_id());
    let (out, _) = ok(&mcp, "exec", json!({ "device": agent.device_id(), "command": "echo hi" })).await;
    assert_eq!(out["stdout"], "hi\n");
    mcp.cancel().await.unwrap();
}
