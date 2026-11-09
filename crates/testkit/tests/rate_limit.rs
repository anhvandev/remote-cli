//! Wrong Device Passwords are rate-limited per (Device ID, IP): 5 failures lock the pair out for a minute, and each
//! later lockout doubles up to an hour. Every failure and lockout is audited (tested below as the slices land).

use rcli_testkit::{Clock, StatusCode, TestSystem};

#[tokio::test]
async fn five_wrong_passwords_lock_the_device_and_ip_out_for_a_minute() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let path = format!("/api/v1/devices/{}", agent.device_id());
    let wrong = sys.device_client(agent.device_id(), "wrongpassword");

    for _ in 0..5 {
        let (status, body) = wrong.get(&path).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }

    // The correct password is refused too, with a typed error saying when the lockout ends.
    let right = sys.device_client(agent.device_id(), &agent.device_password().unwrap());
    let (status, body) = right.get(&path).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], "too_many_attempts");
    let until: chrono::DateTime<chrono::Utc> = body["error"]["locked_until"].as_str().unwrap().parse().unwrap();
    assert_eq!(until, sys.clock.now() + chrono::Duration::minutes(1));
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("60s"), "the remaining time is in the message: {message}");

    sys.advance(61);
    let (status, body) = right.get(&path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn each_later_lockout_doubles_up_to_an_hour() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let path = format!("/api/v1/devices/{}", agent.device_id());
    let wrong = sys.device_client(agent.device_id(), "wrongpassword");
    let right = sys.device_client(agent.device_id(), &agent.device_password().unwrap());

    // 1, 2, 4, 8, 16, 32 minutes, then the cap at one hour.
    for minutes in [1i64, 2, 4, 8, 16, 32, 60] {
        for _ in 0..5 {
            let (status, body) = wrong.get(&path).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        }
        let (status, body) = right.get(&path).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        let until: chrono::DateTime<chrono::Utc> = body["error"]["locked_until"].as_str().unwrap().parse().unwrap();
        assert_eq!(until, sys.clock.now() + chrono::Duration::minutes(minutes), "{body}");
        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains(&format!("{}s", minutes * 60)), "{message}");
        sys.advance(minutes * 60 + 1);
    }
}

#[tokio::test]
async fn another_device_or_another_ip_is_unaffected() {
    let sys = TestSystem::start_dual_stack().await;
    let mine = sys.start_agent("mine").await;
    let other = sys.start_agent("other").await;
    sys.wait_device(mine.device_id(), |d| d["status"] == "Online").await;
    sys.wait_device(other.device_id(), |d| d["status"] == "Online").await;
    let path = format!("/api/v1/devices/{}", mine.device_id());
    let wrong = sys.device_client(mine.device_id(), "wrongpassword");
    for _ in 0..5 {
        wrong.get(&path).await;
    }

    // The same Device from another IP (the Server's `::1` side), and another Device from the locked-out IP,
    // both still work.
    let elsewhere = format!("http://[::1]:{}", sys.server.http_addr.port());
    let from_elsewhere = sys
        .device_client(mine.device_id(), &mine.device_password().unwrap())
        .with_base(&elsewhere);
    let (status, body) = from_elsewhere.get(&path).await;
    assert_eq!(status, StatusCode::OK, "another IP: {body}");

    let other_path = format!("/api/v1/devices/{}", other.device_id());
    let other_device = sys.device_client(other.device_id(), &other.device_password().unwrap());
    let (status, body) = other_device.get(&other_path).await;
    assert_eq!(status, StatusCode::OK, "another Device: {body}");
}

#[tokio::test]
async fn every_failure_and_lockout_is_audited() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let dev = agent.device_id().to_owned();
    let path = format!("/api/v1/devices/{dev}");
    let wrong = sys.device_client(&dev, "wrongpassword").with_controller("dev-laptop");
    for _ in 0..5 {
        wrong.get(&path).await;
    }

    let (status, entries) = sys.api.get("/api/v1/audit").await;
    assert_eq!(status, StatusCode::OK, "{entries}");
    let rows: Vec<_> = entries.as_array().unwrap().iter().filter(|e| e["user"] == "dev-laptop (127.0.0.1)").collect();
    let failures: Vec<_> = rows.iter().filter(|e| e["action"] == "connect.failed").collect();
    assert_eq!(failures.len(), 5, "{rows:?}");
    assert_eq!(failures[0]["device_id"], dev);
    assert_eq!(failures[0]["result"]["error"], "unauthorized");
    let locked: Vec<_> = rows.iter().filter(|e| e["action"] == "connect.locked").collect();
    assert_eq!(locked.len(), 1, "{rows:?}");
    let until: chrono::DateTime<chrono::Utc> = locked[0]["result"]["locked_until"].as_str().unwrap().parse().unwrap();
    assert_eq!(until, sys.clock.now() + chrono::Duration::minutes(1));
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::path::PathBuf::from(format!("/tmp/rcli04-{}-{name}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn the_cli_says_how_long_the_lockout_lasts() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let dir = temp_dir("lockout");
    let server = format!("http://{}", sys.server.http_addr);
    sys.rcli_in(&dir, &["setup", "--server", &server, "--key", &sys.server_key()]).await.unwrap();
    for _ in 0..5 {
        sys.rcli_in(&dir, &["connect", agent.device_id(), "--password", "wrongpassword"]).await.unwrap_err();
    }

    let err = sys.rcli_in(&dir, &["connect", agent.device_id(), "--password", &agent.device_password().unwrap()]).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Device Password"), "{message}");
    assert!(message.contains("60s"), "the remaining time is in the message: {message}");
}

#[tokio::test]
async fn a_wrong_device_id_is_indistinguishable_and_does_not_bypass_the_limiter() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;

    // An unknown Device ID answers exactly like a wrong password for a real one: no way to tell they differ.
    let ghost = sys.device_client("999999999", "whatever");
    let (status, body) = ghost.get("/api/v1/devices/999999999").await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::UNAUTHORIZED, serde_json::json!("unauthorized")));

    // An unknown Device ID is counted like a real one, so guessing one is limited; it never locks another Device.
    for _ in 0..5 {
        ghost.get("/api/v1/devices/999999999").await;
    }
    let (status, body) = ghost.get("/api/v1/devices/999999999").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");

    let real = sys.device_client(agent.device_id(), &agent.device_password().unwrap());
    let (status, body) = real.get(&format!("/api/v1/devices/{}", agent.device_id())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn parallel_guesses_still_reach_the_lockout() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    sys.wait_device(agent.device_id(), |d| d["status"] == "Online").await;
    let path = format!("/api/v1/devices/{}", agent.device_id());
    let wrong = sys.device_client(agent.device_id(), "wrongpassword");

    // Ten guesses at once count as ten failures, not one: a parallel guesser still hits the lockout.
    let all = futures::future::join_all((0..10).map(|_| wrong.get(&path))).await;
    assert!(all.iter().all(|(status, _)| *status == StatusCode::UNAUTHORIZED));

    let right = sys.device_client(agent.device_id(), &agent.device_password().unwrap());
    let (status, body) = right.get(&path).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
}
