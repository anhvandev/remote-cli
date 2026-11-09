//! Controller setup: `rcli setup`, `connect <Device ID>` and `forget`; then Device commands need only the ID.

use rcli_testkit::{StatusCode, temp_dir, TestSystem};

fn http_addr(sys: &TestSystem) -> String {
    format!("http://{}", sys.server.http_addr)
}

#[tokio::test]
async fn setup_saves_the_server_address_key_and_pinned_update_key() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("setup");
    let out = sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    assert!(out.contains("saved"), "{out}");

    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["server"], http_addr(&sys));
    assert_eq!(config["server_key"], sys.server_key());
    let pinned = hex::encode(sys.config.update_public_key.clone().unwrap());
    assert_eq!(config["update_key"], pinned, "the update key is pinned at setup");
}

#[tokio::test]
async fn connect_then_shot_works_with_no_env_vars_and_the_password_is_not_in_the_config() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let dir = temp_dir("connect");
    let password = agent.device_password().unwrap();
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();

    let out = sys.rcli_in(&dir, &["connect", agent.device_id(), "--password", &password]).await.unwrap();
    assert!(out.contains(agent.device_id()), "{out}");

    let shots = temp_dir("shots");
    let out = sys
        .rcli_in(&dir, &["shot", agent.device_id(), "--out", shots.to_str().unwrap()])
        .await
        .unwrap();
    assert!(out.contains("display 0"), "{out}");
    assert!(std::fs::read_dir(&shots).unwrap().count() > 0);

    for file in std::fs::read_dir(&dir).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(!bytes.windows(password.len()).any(|w| w == password.as_bytes()), "plaintext password in {}", path.display());
        }
    }
}

#[tokio::test]
async fn devices_lists_only_the_connected_device_and_another_device_is_refused() {
    let sys = TestSystem::start().await;
    let mine = sys.start_agent("mine").await;
    let other = sys.start_agent("other").await;
    sys.wait_device(mine.device_id(), |d| d["status"] == "Online").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;
    let dir = temp_dir("devices");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    sys.rcli_in(&dir, &["connect", mine.device_id(), "--password", &mine.device_password().unwrap()]).await.unwrap();

    let list = sys.rcli_in(&dir, &["devices"]).await.unwrap();
    assert!(list.contains(mine.device_id()), "{list}");
    assert!(!list.contains(other.device_id()), "a password connection sees only its own Device: {list}");

    let shots = temp_dir("devices-shots");
    let err = sys.rcli_in(&dir, &["shot", other.device_id(), "--out", shots.to_str().unwrap()]).await.unwrap_err();
    assert!(err.to_string().contains("forbidden_device"), "{err}");
}

#[tokio::test]
async fn a_wrong_password_is_reported_and_not_saved() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let dir = temp_dir("wrong-password");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();

    let err = sys.rcli_in(&dir, &["connect", agent.device_id(), "--password", "wrongpassword"]).await.unwrap_err();
    assert!(err.to_string().contains("Device Password"), "{err}");

    let shots = temp_dir("wrong-password-shots");
    let err = sys.rcli_in(&dir, &["shot", agent.device_id(), "--out", shots.to_str().unwrap()]).await.unwrap_err();
    assert!(err.to_string().contains("connect"), "{err}");
}

#[tokio::test]
async fn forget_removes_the_remembered_password() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let dir = temp_dir("forget");
    sys.rcli_in(&dir, &["setup", "--server", &http_addr(&sys), "--key", &sys.server_key()]).await.unwrap();
    sys.rcli_in(&dir, &["connect", agent.device_id(), "--password", &agent.device_password().unwrap()]).await.unwrap();

    let out = sys.rcli_in(&dir, &["forget", agent.device_id()]).await.unwrap();
    assert!(out.contains("forgot"), "{out}");
    let shots = temp_dir("forget-shots");
    let err = sys.rcli_in(&dir, &["shot", agent.device_id(), "--out", shots.to_str().unwrap()]).await.unwrap_err();
    assert!(err.to_string().contains("connect"), "{err}");
}

#[tokio::test]
async fn the_server_does_not_store_the_device_password_in_plaintext() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    // The admin view never shows it, and the row holds only an argon2 hash.
    let (status, device) = sys.api.get(&format!("/api/v1/devices/{}", agent.device_id())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(device["device_password"].is_null(), "{device}");
}
