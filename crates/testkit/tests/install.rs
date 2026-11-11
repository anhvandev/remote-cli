//! `rcli install` makes a machine both a Device and a Controller (ADR 0006): it registers the Device, prints its
//! Device ID and Device Password, and writes the Controller configuration machine-wide, so every local user can run
//! `rcli connect` and `rcli mcp` at once. Each test installs into a temp directory (`rcli --root`), not `/`.

use rcli_testkit::rmcp::model::CallToolRequestParams;
use rcli_testkit::{temp_dir, TestSystem};
use serde_json::{json, Value};

/// The API Address an Admin set to this test Server (plain http is allowed only to loopback, as for the CLI).
async fn set_api_address(sys: &TestSystem) -> String {
    let url = format!("http://{}", sys.server.http_addr);
    sys.rcli(&["api-address", "set", &url]).await.unwrap();
    url
}

fn printed<'a>(out: &'a str, label: &str) -> &'a str {
    out.lines().find_map(|l| l.strip_prefix(label)).unwrap_or_else(|| panic!("no {label} in {out}")).trim()
}

#[tokio::test]
async fn after_install_every_local_user_connects_and_runs_mcp_with_no_setup() {
    let sys = TestSystem::start().await;
    set_api_address(&sys).await;
    let target = sys.start_agent("lab-pc").await;
    sys.wait_device(target.device_id(), |d| d["status"] == "Online").await;
    let password = target.device_password().unwrap();

    let machine = temp_dir("desk");
    let out = sys.install(&machine).await.unwrap();
    // The machine is a Device too: the printed Device ID comes Online once its Agent runs.
    let device_id = printed(&out, "Device ID:").to_owned();
    assert!(printed(&out, "Device Password:").len() >= 8, "{out}");
    let _agent = sys.start_installed(&machine);
    sys.wait_device(&device_id, |d| d["status"] == "Online").await;

    for user in ["alice", "bob"] {
        let home = temp_dir(user);
        let out = sys.rcli_user(&machine, &home, &["connect", target.device_id(), "--password", &password]).await.unwrap();
        assert!(out.contains(target.device_id()), "{out}");
        let out = sys.rcli_user(&machine, &home, &["exec", target.device_id(), "--", "echo", "hi"]).await.unwrap();
        assert_eq!(out, "hi\n");

        let mcp = sys.mcp_user(&machine, &home).await;
        let args = json!({ "device": target.device_id(), "command": "echo hi" }).as_object().unwrap().clone();
        let r = mcp.call_tool(CallToolRequestParams::new("exec").with_arguments(args)).await.unwrap();
        assert_eq!(r.structured_content.as_ref().map(|v| v["stdout"].clone()), Some(Value::from("hi\n")), "{r:?}");
        mcp.cancel().await.unwrap();
    }
}

#[tokio::test]
async fn with_no_api_address_the_install_succeeds_and_controller_commands_say_why_until_the_agent_learns_it() {
    let sys = TestSystem::start().await;
    let target = sys.start_agent("lab-pc").await;
    sys.wait_device(target.device_id(), |d| d["status"] == "Online").await;
    let password = target.device_password().unwrap();
    let machine = temp_dir("early");
    let out = sys.install(&machine).await.unwrap();
    assert!(out.contains("Device ID:") && out.contains("no API Address yet"), "{out}");
    let mut agent = sys.start_installed(&machine);

    for args in [vec!["connect", target.device_id(), "--password", &password], vec!["mcp"], vec!["devices"]] {
        let err = sys.rcli_in(&machine, &args).await.unwrap_err();
        assert!(err.to_string().contains("the Server has no API Address yet"), "{args:?}: {err}");
    }

    // An Admin sets it; the Agent gets it on its next connection and keeps the machine-wide configuration current.
    set_api_address(&sys).await;
    agent.stop();
    agent.start();
    rcli_testkit::eventually(|| async {
        sys.rcli_in(&machine, &["connect", target.device_id(), "--password", &password]).await.ok()
    })
    .await;
    let out = sys.rcli_in(&machine, &["exec", target.device_id(), "--", "echo", "hi"]).await.unwrap();
    assert_eq!(out, "hi\n");
}

/// Old instructions fail loudly instead of half-working: no alias (ADR 0006).
#[tokio::test]
async fn setup_and_agent_install_and_uninstall_no_longer_exist() {
    let sys = TestSystem::start().await;
    let machine = temp_dir("old-commands");
    let (server, key) = (sys.agent_addr(), sys.server_key());
    for args in [
        vec!["setup", "--server", &server, "--key", &key],
        vec!["agent", "install", "--server", &server, "--key", &key],
        vec!["agent", "uninstall"],
    ] {
        let err = sys.rcli_in(&machine, &args).await.unwrap_err();
        assert!(err.to_string().contains("unrecognized subcommand"), "{args:?}: {err}");
    }
    assert_eq!(files(&machine), Vec::<std::path::PathBuf>::new());
}

/// Every regular file under `dir`.
fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = vec![];
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        match std::fs::symlink_metadata(&path).unwrap().is_dir() {
            true => found.extend(files(&path)),
            false => found.push(path),
        }
    }
    found
}

#[tokio::test]
async fn a_wrong_server_key_fails_the_install_and_leaves_nothing_behind() {
    let sys = TestSystem::start().await;
    set_api_address(&sys).await;
    let machine = temp_dir("typo");
    let (_, secret) = sys.server_key().rsplit_once('_').map(|(f, s)| (f.to_owned(), s.to_owned())).unwrap();
    let wrong = sys.server_key().replace(&secret, &"0".repeat(secret.len()));

    let err = sys.install_with_key(&machine, &wrong).await.unwrap_err();
    assert!(format!("{err:#}").contains("Server Key"), "{err:#}");
    assert_eq!(files(&machine), Vec::<std::path::PathBuf>::new(), "a failed install left files behind");
    let (_, devices) = sys.api.get("/api/v1/devices").await;
    assert_eq!(devices, json!([]), "nothing registered");

    // A retry with the right key installs cleanly.
    let out = sys.install(&machine).await.unwrap();
    assert!(out.contains("Device ID:"), "{out}");
}

#[tokio::test]
async fn uninstall_removes_the_agent_and_the_machine_wide_configuration_and_keeps_remembered_passwords() {
    let sys = TestSystem::start().await;
    set_api_address(&sys).await;
    let target = sys.start_agent("lab-pc").await;
    sys.wait_device(target.device_id(), |d| d["status"] == "Online").await;
    let machine = temp_dir("leaving");
    let out = sys.install(&machine).await.unwrap();
    let device_id = printed(&out, "Device ID:").to_owned();
    let _agent = sys.start_installed(&machine);
    sys.wait_device(&device_id, |d| d["status"] == "Online").await;
    let home = temp_dir("carol");
    sys.rcli_user(&machine, &home, &["connect", target.device_id(), "--password", &target.device_password().unwrap()]).await.unwrap();

    let out = sys.rcli_in(&machine, &["uninstall"]).await.unwrap();
    assert!(!out.contains("warning"), "{out}");

    // The Device is gone from the Server.
    rcli_testkit::eventually(|| async {
        let (status, _) = sys.api.get(&format!("/api/v1/devices/{device_id}")).await;
        (status == rcli_testkit::StatusCode::NOT_FOUND).then_some(())
    })
    .await;
    assert_eq!(files(&machine), Vec::<std::path::PathBuf>::new(), "uninstall left files behind");
    let err = sys.rcli_user(&machine, &home, &["devices"]).await.unwrap_err();
    assert!(err.to_string().contains("not installed"), "{err}");

    // Reinstalling brings Controller commands back with the password Carol remembered.
    sys.install(&machine).await.unwrap();
    let out = sys.rcli_user(&machine, &home, &["exec", target.device_id(), "--", "echo", "hi"]).await.unwrap();
    assert_eq!(out, "hi\n");
}

#[tokio::test]
async fn remembered_device_passwords_stay_with_the_user_who_connected() {
    let sys = TestSystem::start().await;
    set_api_address(&sys).await;
    let target = sys.start_agent("lab-pc").await;
    sys.wait_device(target.device_id(), |d| d["status"] == "Online").await;
    let password = target.device_password().unwrap();
    let machine = temp_dir("shared");
    sys.install(&machine).await.unwrap();
    let (alice, bob) = (temp_dir("alice"), temp_dir("bob"));
    sys.rcli_user(&machine, &alice, &["connect", target.device_id(), "--password", &password]).await.unwrap();

    // Bob shares the install, not Alice's connection.
    let err = sys.rcli_user(&machine, &bob, &["exec", target.device_id(), "--", "echo", "hi"]).await.unwrap_err();
    assert!(err.to_string().contains("rcli connect"), "{err}");

    // The machine-wide file is readable by every local user and holds no Device Password; Alice's files are hers.
    let config = rcli::config::machine_path(Some(&machine));
    let bytes = std::fs::read(&config).unwrap();
    assert!(!bytes.windows(password.len()).any(|w| w == password.as_bytes()), "a Device Password in the machine-wide file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&config), 0o644, "read-only for local users");
        for file in std::fs::read_dir(alice.join("passwords")).unwrap() {
            let path = file.unwrap().path();
            assert_eq!(mode(&path), 0o600, "{} is readable by other local users", path.display());
        }
    }
}
