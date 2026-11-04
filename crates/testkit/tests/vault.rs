use rcli_testkit::{FakePlatform, Input, SessionState, StatusCode, TestSystem};
use serde_json::json;

const PASSWORD: &str = "hunter2-very-secret";

#[tokio::test]
async fn unlock_types_the_stored_password_without_ever_revealing_it() {
    let sys = TestSystem::start().await;
    let platform = FakePlatform::new("lab-pc");
    platform.set_session_state(SessionState::Locked);
    let agent = sys.start_agent_on(platform).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let alice_token = sys.create_user("alice", "operator").await;
    let alice = sys.api.with_token(Some(&alice_token));
    let pw_path = format!("/api/v1/devices/{dev}/unlock-password");
    let mut seen = Vec::new(); // every Controller-visible response

    // Only admins store passwords.
    let (status, body) = alice.post_json(&pw_path, &json!({ "password": PASSWORD })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let out = sys.rcli(&["unlock-password", "set", "lab-pc", "--password", PASSWORD]).await.unwrap();
    seen.push(out);
    let (status, body) = sys.api.get(&pw_path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["set"], true);
    seen.push(body.to_string());

    // Needs the Control Lock.
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "control_lock_required");
    assert!(agent.platform.inputs().is_empty());

    sys.rcli_as(&alice_token, &["lock", "acquire", "lab-pc"]).await.unwrap();
    let dir = tempdir();
    let out = sys.rcli_as(&alice_token, &["unlock", "lab-pc", "--out", dir.to_str().unwrap()]).await.unwrap();
    assert!(out.contains("display 0"), "{out}");
    seen.push(out);
    assert_eq!(agent.platform.inputs(), vec![Input::Text(PASSWORD.into()), Input::Key("enter".into())]);
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["images"].is_array());

    let (_, entries) = sys.api.get("/api/v1/audit").await;
    let unlocks: Vec<_> = entries.as_array().unwrap().iter().filter(|e| e["action"] == "unlock").collect();
    assert_eq!(unlocks.len(), 2, "{entries}");
    assert_eq!(unlocks[0]["user"], "alice");
    assert_eq!(unlocks[0]["result"]["error"], serde_json::Value::Null);
    seen.push(entries.to_string());
    seen.push(sys.rcli(&["audit"]).await.unwrap());
    seen.push(sys.api.get(&format!("/api/v1/devices/{dev}")).await.1.to_string());
    seen.push(sys.api.get("/api/v1/devices").await.1.to_string());
    for s in &seen {
        assert!(!s.contains(PASSWORD), "password leaked: {s}");
    }

    // Cleared: unlock fails clearly and the Agent types nothing.
    sys.rcli(&["unlock-password", "clear", "lab-pc"]).await.unwrap();
    assert_eq!(sys.api.get(&pw_path).await.1["set"], false);
    let (status, body) = alice.post(&format!("/api/v1/devices/{dev}/unlock")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "unlock_password_not_set");
    let err = sys.rcli_as(&alice_token, &["unlock", "lab-pc"]).await.unwrap_err();
    assert!(err.to_string().contains("unlock_password_not_set"), "{err}");
    assert_eq!(agent.platform.inputs().len(), 4, "only the two unlocks above typed anything");
}

#[tokio::test]
async fn a_password_is_bound_to_its_device() {
    let sys = TestSystem::start().await;
    let a = sys.start_agent("a").await;
    let b = sys.start_agent("b").await;
    sys.wait_device(b.device_id(), |d| d["status"] == "Online").await;
    sys.rcli(&["unlock-password", "set", "a", "--password", "pw-a"]).await.unwrap();
    sys.rcli(&["unlock-password", "set", "a", "--password", "pw-a2"]).await.unwrap(); // replaces
    sys.rcli(&["unlock-password", "set", "b", "--password", "pw-b"]).await.unwrap();
    sys.rcli(&["lock", "acquire", "b"]).await.unwrap();
    sys.rcli(&["unlock", "b", "--json"]).await.unwrap();
    assert_eq!(b.platform.inputs()[0], Input::Text("pw-b".into()));
    sys.rcli(&["lock", "acquire", "a"]).await.unwrap();
    sys.rcli(&["unlock", "a", "--json"]).await.unwrap();
    assert_eq!(a.platform.inputs()[0], Input::Text("pw-a2".into()));
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rcli-vault-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
