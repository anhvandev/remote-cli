//! The Agent's local IPC: `rcli agent ...` talks to the running service's socket, not the Server.

use rcli_testkit::{eventually, temp_dir, TestSystem};

#[tokio::test]
async fn the_device_password_is_ten_characters_without_look_alikes() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("password");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let password = agent.device_password().expect("a Device Password is generated at registration");
    assert_eq!(password.chars().count(), 10, "{password}");
    assert!(!password.chars().any(|c| "0O1lI".contains(c)), "{password}");
    assert!(password.chars().all(|c| c.is_ascii_alphanumeric()), "{password}");
}

#[tokio::test]
async fn the_password_is_not_stored_in_plaintext_on_the_device() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("password-at-rest");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let password = agent.device_password().unwrap();
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue; // the IPC socket
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(password.len()).any(|w| w == password.as_bytes()), "plaintext password in {}", path.display());
        checked += 1;
    }
    assert!(checked >= 2, "the Secret Store key and the sealed password exist");
}

#[tokio::test]
async fn password_show_is_refused_to_non_root() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("password-show");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    rcli_testkit::eventually(|| async { socket.exists().then_some(()) }).await;
    let out = sys.agent_rcli(&socket, &["agent", "password", "show"]).await;
    if rcli_testkit::running_as_root() {
        // Root may read it; that is the documented escape hatch, not the rule under test.
        assert_eq!(out.unwrap().trim(), agent.device_password().unwrap());
    } else {
        let err = out.unwrap_err();
        assert!(err.to_string().contains("root"), "{err}");
    }
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
