//! Wrong Device Passwords are rate-limited per (Device ID, IP): 5 failures lock the pair out for a minute, and each
//! later lockout doubles up to an hour. Every failure and lockout is audited (tested below as the slices land).

use rcli_testkit::{StatusCode, TestSystem};

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
