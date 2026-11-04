use rcli_testkit::{
    platform::{Display, Inventory, Process, Resources, SessionState},
    TestSystem,
};

#[tokio::test]
async fn device_details_show_inventory_and_follow_its_changes() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    agent.platform.update(|s| {
        s.inventory = Inventory {
            os: "Windows".into(),
            os_version: "11 23H2".into(),
            hostname: "LAB-PC".into(),
            cpu_model: "Ryzen 7 5800X".into(),
            cpu_cores: 8,
            ram_bytes: 32 << 30,
            displays: vec![Display { name: "DP-1".into(), width: 2560, height: 1440 }],
            ..Default::default()
        }
    });

    let device = sys.wait_device_details(agent.device_id(), |d| d["inventory"]["os"] == "Windows").await;
    assert_eq!(device["name"], "lab-pc");
    assert_eq!(device["inventory"]["cpu_cores"], 8);
    assert_eq!(device["inventory"]["displays"][0]["width"], 2560);

    agent.platform.update(|s| s.inventory.displays[0].width = 3840);
    sys.wait_device_details(agent.device_id(), |d| d["inventory"]["displays"][0]["width"] == 3840).await;
}

#[tokio::test]
async fn rcli_device_show_reports_current_resources_and_lock_screen() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    agent.platform.update(|s| {
        s.resources = Resources {
            cpu_percent: Some(12.5),
            ram_used_bytes: Some(8 << 30),
            disk_used_percent: [("C:".to_string(), 40.0)].into(),
            uptime_secs: Some(3600),
            logged_in_user: Some("alice".into()),
            session: Some(SessionState::Active.into()),
            battery_percent: Some(80.0),
            ..Default::default()
        }
    });
    sys.wait_device_details(agent.device_id(), |d| d["resources"]["cpu_percent"] == 12.5).await;

    // Only the changed fields travel; the rest must still be there.
    agent.platform.update(|s| {
        s.resources.cpu_percent = Some(97.0);
        s.resources.session = Some(SessionState::LockScreen.into());
    });
    sys.wait_device_details(agent.device_id(), |d| d["resources"]["session"] == "LockScreen").await;

    let json: serde_json::Value =
        serde_json::from_str(&sys.rcli(&["device", "show", "lab-pc", "--json"]).await.unwrap()).unwrap();
    assert_eq!(json["id"], agent.device_id());
    assert_eq!(json["status"], "Online");
    let r = &json["resources"];
    assert_eq!(r["cpu_percent"], 97.0);
    assert_eq!(r["ram_used_bytes"], 8u64 << 30);
    assert_eq!(r["disk_used_percent"]["C:"], 40.0);
    assert_eq!(r["logged_in_user"], "alice");
    assert_eq!(r["battery_percent"], 80.0);
    assert!(r["cpu_temp_celsius"].is_null(), "unavailable values are null: {r}");
    assert_eq!(json["inventory"]["os"], "FakeOS");

    let text = sys.rcli(&["device", "show", agent.device_id()]).await.unwrap();
    for want in ["lab-pc", "Online", "FakeOS", "97.0", "LockScreen", "alice"] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
}

#[tokio::test]
async fn rcli_device_processes_asks_the_agent_on_demand() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    agent.platform.update(|s| {
        s.processes = vec![Process {
            pid: 4242,
            name: "blender".into(),
            user: "alice".into(),
            cpu_percent: 55.5,
            memory_bytes: 1 << 30,
        }]
    });

    let json: serde_json::Value =
        serde_json::from_str(&sys.rcli(&["device", "processes", "lab-pc", "--json"]).await.unwrap()).unwrap();
    assert_eq!(json[0]["pid"], 4242);
    assert_eq!(json[0]["name"], "blender");
    let text = sys.rcli(&["device", "processes", agent.device_id()]).await.unwrap();
    let row = text.lines().find(|l| l.contains("blender")).expect(&text);
    assert!(row.contains("4242") && row.contains("alice") && row.contains("55.5"), "{text}");

    agent.stop();
    sys.wait_device(agent.device_id(), |d| !d["last_seen_at"].is_null()).await;
    let err = sys.rcli(&["device", "processes", "lab-pc"]).await.unwrap_err();
    assert!(err.to_string().contains("device_not_connected"), "{err}");
    let err = sys.rcli(&["device", "show", "no-such-pc"]).await.unwrap_err();
    assert!(err.to_string().contains("device_not_found"), "{err}");
}
