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

    let tools: Vec<String> = mcp.list_all_tools().await.unwrap().into_iter().map(|t| t.name.into_owned()).collect();
    for t in ["devices", "device", "lock_acquire", "lock_release", "lock_status", "act", "batch", "observe", "exec", "wake"] {
        assert!(tools.contains(&t.to_owned()), "missing tool {t}: {tools:?}");
    }

    let (devices, _) = ok(&mcp, "devices", json!({})).await;
    assert_eq!(devices["devices"][0]["name"], "lab-pc", "{devices}");
    let (details, _) = ok(&mcp, "device", json!({ "device": "lab-pc" })).await;
    assert_eq!(details["id"], dev);

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
async fn a_held_control_lock_is_a_readable_tool_error() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let bob = sys.create_user("bob", "operator").await;
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
