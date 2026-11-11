//! Ticket 07: the Terminal User and root access Device Settings.

use std::time::Duration;

use rcli_testkit::{eventually, FakePlatform, online, SessionState, StatusCode, temp_dir, TestSystem};
use serde_json::json;

#[tokio::test]
async fn exec_and_shell_run_as_the_terminal_user() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("terminal-user");
    let agent = sys.start_agent_trusting_socket("lab-pc", &dir).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;

    // The default is the account running the Agent (this test process).
    let running = std::env::var("USER").expect("USER");
    let (status, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "whoami" })).await;
    assert_eq!(status, rcli_testkit::StatusCode::OK, "{out}");
    assert_eq!(out["stdout"], format!("{running}\n"));

    sys.agent_rcli(&socket, &["agent", "terminal-user", "set", "alice"]).await.unwrap();

    let (_, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "whoami" })).await;
    assert_eq!(out["stdout"], "alice\n", "{out}");
    // `sudo` inside the terminal still works, as over SSH.
    let (_, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &serde_json::json!({ "command": "sudo whoami" })).await;
    assert_eq!(out["stdout"], "root\n", "{out}");
}

/// Runs `whoami --as root` and returns the response.
async fn root_whoami(sys: &TestSystem, token: &str, device: &str) -> (StatusCode, serde_json::Value) {
    sys.api
        .with_token(Some(token))
        .post_json(&format!("/api/v1/devices/{device}/exec"), &json!({ "command": "whoami", "as": "root" }))
        .await
}

async fn audit(sys: &TestSystem) -> Vec<serde_json::Value> {
    let (status, body) = sys.api.get("/api/v1/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().iter().filter(|e| e["action"] == "terminal").cloned().collect()
}

#[tokio::test]
async fn ask_rejects_when_no_session_can_show_the_prompt() {
    let sys = TestSystem::start().await;
    let platform = FakePlatform::new("lab-pc");
    platform.set_session_state(SessionState::LoginScreen);
    let (agent, _dir) = online(&sys, "ask-no-session", platform.clone()).await;
    let dev = agent.device_id().to_owned();

    // `ask` is the default; with nobody logged in, nobody can accept.
    let (status, out) = root_whoami(&sys, &sys.admin_token, &dev).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("root_denied")), "{out}");
    assert!(platform.root_prompts().is_empty(), "no prompt was shown");
    assert_eq!(audit(&sys).await[0]["result"]["error"], "root_denied");
}

#[tokio::test]
async fn ask_accept_reject_and_the_thirty_second_timeout() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "ask", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let socket = dir.join("agent.sock");
    let alice = sys.create_user("alice", "operator").await;

    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("root_denied")), "{out}");

    agent.platform.answer_root_prompts([true]);
    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!((&out["stdout"], &out["as"], &out["root_decision"]), (&json!("root\n"), &json!("root"), &json!("accepted")));

    agent.platform.answer_root_prompts([false]);
    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("root_denied")), "{out}");

    // The prompt names the requester, and the Agent gave it 30 s.
    let prompts = agent.platform.root_prompts();
    assert_eq!(prompts.len(), 3, "{prompts:?}");
    assert_eq!(prompts[0].0, "alice", "a Dashboard User is named by name");
    assert_eq!(prompts[0].1, Duration::from_secs(30));
    let _ = socket;
}

#[tokio::test]
async fn a_password_connection_is_named_by_machine_and_ip_in_the_prompt() {
    let sys = TestSystem::start().await;
    let (agent, _dir) = online(&sys, "ask-password", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let password = agent.device_password().unwrap();
    agent.platform.answer_root_prompts([true]);

    // Rules apply equally to password connections; a device_password-only client can call exec.
    let device = sys.device_client(&dev, &password);
    let (status, out) = device.post_json(&format!("/api/v1/devices/{dev}/exec"), &json!({ "command": "whoami", "as": "root" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["as"], "root", "{out}");
    let prompts = agent.platform.root_prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].0.starts_with("test-controller ("), "{}", prompts[0].0);
}

#[tokio::test]
async fn never_forbids_root_whether_signed_in_or_not() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "never", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let socket = dir.join("agent.sock");
    sys.agent_rcli(&socket, &["agent", "root", "never"]).await.unwrap();

    let (status, out) = root_whoami(&sys, &sys.admin_token, &dev).await;
    assert_eq!((status, &out["error"]["code"]), (StatusCode::FORBIDDEN, &json!("root_denied")), "{out}");
    assert!(agent.platform.root_prompts().is_empty());
}

#[tokio::test]
async fn a_session_grant_lasts_until_the_session_ends() {
    let sys = TestSystem::start().await;
    let (mut agent, dir) = online(&sys, "grant", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let socket = dir.join("agent.sock");

    // The person at the Device grants root to alice for this session, without changing the setting.
    sys.agent_rcli(&socket, &["agent", "root", "grant", "alice"]).await.unwrap();
    let alice = sys.create_user("alice", "operator").await;
    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["root_decision"], "session", "{out}");
    assert!(agent.platform.root_prompts().is_empty(), "a grant needs no prompt");
    // Someone else still asks.
    let bob = sys.create_user("bob", "operator").await;
    let (status, _) = root_whoami(&sys, &bob, &dev).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The grant does not outlive the session (the Agent's connection to the Server).
    agent.stop();
    // A fresh Agent for the same Device: a new session, as after a restart or a reconnect.
    let mut restarted = sys.resume_agent_on(agent, FakePlatform::new("lab-pc"), &dir).await;
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    assert_eq!(root_whoami(&sys, &alice, &dev).await.0, StatusCode::FORBIDDEN, "the grant ended with the session");
    restarted.stop();
}

/// A password connection names its own machine, so it cannot pick a name that borrows a Dashboard User's grant.
#[tokio::test]
async fn a_password_connection_cannot_borrow_a_grant_by_its_machine_name() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "grant-spoof", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let password = agent.device_password().unwrap();
    let socket = dir.join("agent.sock");

    // The person grants alice by either name the GUI and docs use; a machine named like her gets nothing from it.
    for host in ["alice", "alice (Dashboard)"] {
        sys.agent_rcli(&socket, &["agent", "root", "grant", host]).await.unwrap();
        let device = sys.device_client(&dev, &password).with_controller(host);
        let (status, out) =
            device.post_json(&format!("/api/v1/devices/{dev}/exec"), &json!({ "command": "whoami", "as": "root" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}: {out}");
    }
    assert_eq!(agent.platform.root_prompts().len(), 2, "asked instead of using alice's grant");
    assert!(agent.platform.root_prompts().iter().all(|(who, _)| !who.contains("(Dashboard)")), "{:?}", agent.platform.root_prompts());
}

/// A disconnected Controller loses its root grant: reconnecting with the password does not bring root back.
#[tokio::test]
async fn a_session_grant_ends_when_its_controller_is_disconnected() {
    let sys = TestSystem::start().await;
    let (agent, dir) = online(&sys, "grant-disconnect", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let socket = dir.join("agent.sock");
    let alice = sys.create_user("alice", "operator").await;

    // The GUI grants by the name it lists: "alice (Dashboard)".
    sys.agent_rcli(&socket, &["agent", "root", "grant", "alice (Dashboard)"]).await.unwrap();
    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!((status, &out["root_decision"]), (StatusCode::OK, &json!("session")), "{out}");

    sys.agent_rcli(&socket, &["agent", "disconnect", "alice"]).await.unwrap();
    let (status, out) = root_whoami(&sys, &alice, &dev).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{out}");
    assert_eq!(agent.platform.root_prompts().len(), 1, "asked again instead of using the old grant");
}

#[tokio::test]
async fn rcli_root_on_makes_exec_request_root_by_default() {
    let sys = TestSystem::start().await;
    let (agent, _dir) = online(&sys, "controller-root", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let _alice = sys.create_user("alice", "operator").await;
    let config_dir = temp_dir("controller-root-config");
    // A Controller that connected with the Device Password, as `rcli setup` + `rcli connect` leave it.
    sys.controller_config(&config_dir, Some((&dev, &agent.device_password().unwrap()))).await;

    // Without `root on`, plain exec is not root.
    let out = sys.rcli_in(&config_dir, &["exec", &dev, "--", "whoami"]).await.unwrap();
    assert_eq!(out.trim(), std::env::var("USER").unwrap());

    // With it, exec requests root (the Device still decides: `ask` prompts and this Device accepts).
    agent.platform.answer_root_prompts([true]);
    sys.rcli_in(&config_dir, &["root", "on", &dev]).await.unwrap();
    let out = sys.rcli_in(&config_dir, &["exec", &dev, "--", "whoami"]).await.unwrap();
    assert_eq!(out, "root\n");
    assert_eq!(agent.platform.root_prompts().len(), 1);

    // `rcli root off` reverts to the Terminal User.
    sys.rcli_in(&config_dir, &["root", "off", &dev]).await.unwrap();
    let out = sys.rcli_in(&config_dir, &["exec", &dev, "--", "whoami"]).await.unwrap();
    assert_eq!(out.trim(), std::env::var("USER").unwrap());
}

#[tokio::test]
async fn every_root_run_and_prompt_answer_is_audited() {
    let sys = TestSystem::start().await;
    let (agent, _dir) = online(&sys, "audit", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    let alice = sys.create_user("alice", "operator").await;

    // accepted
    agent.platform.answer_root_prompts([true]);
    assert_eq!(root_whoami(&sys, &alice, &dev).await.0, StatusCode::OK);
    // rejected
    agent.platform.answer_root_prompts([false]);
    assert_eq!(root_whoami(&sys, &alice, &dev).await.0, StatusCode::FORBIDDEN);

    let entries = audit(&sys).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0]["user"], "alice");
    assert_eq!(entries[0]["params"]["as"], "root");
    assert_eq!(entries[0]["result"]["as"], "root");
    assert_eq!(entries[0]["result"]["root_decision"], "accepted");
    // A refused prompt is still an answer worth recording.
    assert_eq!(entries[1]["params"]["as"], "root");
    assert_eq!(entries[1]["result"]["error"], "root_denied");
}

#[tokio::test]
async fn the_terminal_user_setting_survives_a_restart() {
    let sys = TestSystem::start().await;
    let (mut agent, dir) = online(&sys, "terminal-user-persisted", FakePlatform::new("lab-pc")).await;
    let dev = agent.device_id().to_owned();
    sys.agent_rcli(&dir.join("agent.sock"), &["agent", "terminal-user", "set", "alice"]).await.unwrap();

    // The setting is in the Device Settings file next to the credentials, so a restart keeps it.
    let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["terminal_user"], "alice");
    agent.stop();
    let mut restarted = sys.resume_agent_on(agent, FakePlatform::new("lab-pc"), &dir).await;
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let (status, out) = sys.api.post_json(&format!("/api/v1/devices/{dev}/exec"), &json!({ "command": "whoami" })).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["stdout"], "alice\n");
    restarted.stop();
}

#[tokio::test]
async fn only_root_can_change_the_terminal_user() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("terminal-user-gate");
    let agent = sys.start_agent_with_socket("lab-pc", &dir).await;
    let socket = dir.join("agent.sock");
    eventually(|| async { socket.exists().then_some(()) }).await;
    let out = sys.agent_rcli(&socket, &["agent", "terminal-user", "set", "alice"]).await;
    if rcli_testkit::running_as_root() {
        out.unwrap();
    } else {
        assert!(out.unwrap_err().to_string().contains("root"), "a non-root caller must be refused");
    }
    let _ = agent;
}
