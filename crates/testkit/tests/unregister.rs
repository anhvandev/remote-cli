//! A Device removes itself from the Server when its Agent is uninstalled (`sudo rcli agent uninstall` on that
//! Device): the Server treats it as an Admin's Device delete, audits it, and closes the connection. The Agent
//! asks before it deletes its own credentials, so the Device ID and password never outlive the install.

use rcli_testkit::{eventually, StatusCode, TestSystem};

#[tokio::test]
async fn an_agent_removes_its_own_device_and_the_server_audits_it() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("old-box").await;
    let id = agent.device_id().to_owned();
    sys.wait_device(&id, |d| d["status"] == "Online").await;

    // A second Device is untouched.
    let other = sys.start_agent("kept-box").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;

    // What `rcli agent uninstall` does before it deletes the local credentials.
    rcli_agent::unregister(&agent.server_access, &agent.creds).await.unwrap();

    // Gone from the fleet, its detail 404s; the Server closed its connection.
    eventually(|| async {
        let (_, devices) = sys.api.get("/api/v1/devices").await;
        (!devices.as_array()?.iter().any(|d| d["device_id"] == id)).then_some(())
    })
    .await;
    let (status, body) = sys.api.get(&format!("/api/v1/devices/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;

    // Audited as the Admin device delete, naming the Device before its row is gone.
    let (_, audit) = sys.api.get("/api/v1/audit").await;
    let entry = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "device.delete" && e["device_id"] == id)
        .unwrap_or_else(|| panic!("no self-removal audit entry: {audit}"));
    assert_eq!(entry["params"]["name"], "old-box", "{entry}");
    assert_eq!(entry["params"]["self"], true, "{entry}");
    assert!(entry["user"].as_str().unwrap().contains("uninstall"), "{entry}");

    // The Agent's old credentials can no longer connect: a reconnect is refused and registers nothing.
    agent.stop();
    let err = rcli_agent::unregister(&agent.server_access, &agent.creds).await;
    assert!(err.is_ok(), "removing an already removed Device is not an error: {err:?}");
}
