//! The Agent's local IPC: `rcli agent ...` talks to the running service's socket, not the Server.

use rcli_testkit::{eventually, TestSystem};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("rcli-ipc-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn agent_info_shows_the_device_id_and_connection_status_to_any_local_user() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("info");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    let socket = dir.join("agent.sock");
    let out = eventually(|| async {
        let out = sys.agent_rcli(&socket, &["agent", "info"]).await.ok()?;
        out.contains("connected").then_some(out)
    })
    .await;
    assert!(out.contains(agent.device_id()), "{out}");

    let json: serde_json::Value = serde_json::from_str(&sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.unwrap()).unwrap();
    assert_eq!(json["device_id"], agent.device_id());
    assert_eq!(json["connected"], true);
}
