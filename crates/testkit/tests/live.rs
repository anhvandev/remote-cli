use rcli_testkit::{eventually, DisplayInfo, FakePlatform, LiveViewer, StatusCode, TestSystem};
use serde_json::json;
use webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType;

#[tokio::test]
async fn anyone_can_watch_live_peer_to_peer_and_the_indicator_shows_while_streaming() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let alice = sys.api.with_token(Some(&sys.create_user("alice", "operator").await));
    let bob = sys.api.with_token(Some(&sys.create_user("bob", "operator").await));
    assert!(!agent.platform.control_indicator());

    let live = LiveViewer::open(&bob, dev, false).await.expect("open live view");
    live.wait_packets(10).await;
    assert_eq!(live.codec(), "video/H264");
    assert_eq!(live.local_candidate_type().await, RTCIceCandidateType::Host, "direct path, no relay configured");
    assert!(agent.platform.control_indicator(), "indicator shows while streaming");

    // Someone else holding the Control Lock does not stop anyone from watching.
    let lock = format!("/api/v1/devices/{dev}/lock");
    assert_eq!(alice.post(&lock).await.0, StatusCode::OK);
    let second = LiveViewer::open(&bob, dev, false).await.expect("watch while alice holds the lock");
    second.wait_packets(10).await;

    // The indicator stays while the lock is held, and goes once both the streams and the lock are gone.
    assert_eq!(live.close(&bob).await, StatusCode::NO_CONTENT);
    assert_eq!(second.close(&bob).await, StatusCode::NO_CONTENT);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(agent.platform.control_indicator(), "Control Lock still held");
    assert_eq!(alice.post(&format!("{lock}/release")).await.0, StatusCode::OK);
    eventually(|| async { (!agent.platform.control_indicator()).then_some(()) }).await;
    assert!(agent.platform.inputs().is_empty(), "watching sends no input");
}

#[tokio::test]
async fn live_view_falls_back_to_turn_with_short_lived_credentials() {
    let sys = TestSystem::start_with_turn().await;
    let agent = sys.start_agent("behind-nat").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let (status, servers) = sys.api.get("/api/v1/ice-servers").await;
    assert_eq!(status, StatusCode::OK, "{servers}");
    let turn = &servers[0];
    assert_eq!(turn["urls"][0], format!("turn:{}?transport=udp", sys.turn.as_ref().unwrap().addr));
    let expiry: i64 = turn["username"].as_str().unwrap().parse().expect("username = expiry unix time");
    let now = chrono::Utc::now().timestamp();
    assert!(expiry > now && expiry <= now + 24 * 3600, "credentials expire within a day: {turn}");
    assert!(!turn["credential"].as_str().unwrap().is_empty());

    // Relay-only = what happens when NAT blocks every direct path.
    let live = LiveViewer::open(&sys.api, dev, true).await.expect("open live view through TURN");
    live.wait_packets(10).await;
    assert_eq!(live.local_candidate_type().await, RTCIceCandidateType::Relay);
    assert_eq!(live.close(&sys.api).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn live_view_errors_are_typed() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    let live = format!("/api/v1/devices/{dev}/live");

    let (status, body) = sys.api.with_token(None).post_json(&live, &json!({ "sdp": "v=0" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = sys.api.post_json(&live, &json!({ "sdp": "not sdp" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "invalid_offer");
    let (status, body) = sys.api.post_json("/api/v1/devices/nope/live", &json!({ "sdp": "v=0" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    agent.stop();
    sys.wait_device(&dev, |d| !d["last_seen_at"].is_null()).await;
    let err = LiveViewer::open(&sys.api, &dev, false).await.err().expect("refused");
    assert_eq!(err.0, StatusCode::CONFLICT, "{}", err.1);
    assert_eq!(err.1["error"]["code"], "device_not_connected");
}

#[tokio::test]
async fn live_view_streams_native_pixels_while_observations_keep_a_1280_long_edge() {
    let sys = TestSystem::start().await;
    let retina = FakePlatform::new("retina");
    retina.set_displays(vec![
        DisplayInfo { index: 0, x: 0, y: 0, width: 2880, height: 1800, scale: 2.0, primary: true },
        // Odd sizes are rounded down to even for H.264.
        DisplayInfo { index: 1, x: 1440, y: 0, width: 1367, height: 769, scale: 1.0, primary: false },
    ]);
    let agent = sys.start_agent_on(retina).await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let live = LiveViewer::open(&sys.api, dev, false).await.expect("open live view");
    assert_eq!(live.frame_size().await, (2880, 1800), "the primary display's physical pixels");
    live.close(&sys.api).await;
    let live = LiveViewer::open_display(&sys.api, dev, 1).await.expect("open live view of display 1");
    assert_eq!(live.frame_size().await, (1366, 768));
    live.close(&sys.api).await;

    let (status, obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &json!({ "actions": [] })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!((obs["images"][0]["width"].clone(), obs["images"][0]["height"].clone()), (json!(1280), json!(800)));
}
