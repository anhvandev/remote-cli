//! ADR 0005: rcli stores no OS login password. A Controller types it itself with a secret `type` Action.

use rcli_agent::secret_store::{FileSecretStore, SecretStore};
use rcli_testkit::{eventually, temp_dir, StatusCode, TestSystem};

#[tokio::test]
async fn an_upgraded_agent_deletes_a_leftover_unlock_password() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("leftover-unlock");
    let store = FileSecretStore::new(dir.clone());
    store.set("unlock_password", "hunter2-very-secret").unwrap();

    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    eventually(|| async { store.get("unlock_password").unwrap().is_none().then_some(()) }).await;
    assert!(store.get(rcli_agent::ipc::DEVICE_PASSWORD).unwrap().is_some(), "the Device Password stays");
}

#[tokio::test]
async fn there_is_no_unlock_command_endpoint_or_stored_password_flag() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("no-unlock");
    let agent = sys.start_agent_trusting_socket("lab-pc", &dir).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    sys.rcli(&["lock", "acquire", "lab-pc"]).await.unwrap();

    let (status, body) = sys.api.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(sys.rcli(&["unlock", "lab-pc"]).await.is_err());
    assert!(sys.agent_rcli(&socket, &["agent", "unlock-password", "set", "--password", "pw"]).await.is_err());
    let info = sys.agent_rcli(&socket, &["agent", "info", "--json"]).await.unwrap();
    assert!(info.contains("server_key") && !info.contains("unlock"), "{info}");
    assert!(agent.platform.inputs().is_empty());
}
