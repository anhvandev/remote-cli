//! Ticket 26: large Observations and clips from the CLI/MCP travel peer-to-peer (TURN fallback), else via the Server.

use rcli::{media::Media, Api};
use rcli_testkit::{local_candidate_type, StatusCode, TestSystem};
use serde_json::json;
use webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType;

fn api(sys: &TestSystem, token: &str) -> Api {
    Api::new(format!("http://{}", sys.server.http_addr), token.into())
}

#[tokio::test]
async fn large_observations_travel_peer_to_peer_and_match_the_server_relay() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    let media = Media::open(&api(&sys, &sys.admin_token), dev, false).await.expect("media channel");
    assert_eq!(local_candidate_type(media.peer()).await, RTCIceCandidateType::Host);
    let (status, mut obs) = sys.api.post_json(&actions, &json!({ "actions": [], "media_session": media.session })).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    // The Server only saw a reference; the bytes came over the data channel.
    assert!(obs["images"][0]["data"].is_null() && obs["images"][0]["p2p"].is_u64(), "{obs}");
    media.fill(&mut obs).await.unwrap();

    // Another Controller (so nothing is "unchanged") gets the same screen through the Server.
    let bob = sys.create_user("bob", "operator").await;
    let (_, relayed) = sys.api.with_token(Some(&bob)).post_json(&actions, &json!({ "actions": [] })).await;
    assert!(relayed["images"][0]["data"].is_string(), "{relayed}");
    assert_eq!(obs, relayed);
    media.close().await;
}

#[tokio::test]
async fn clips_fall_back_to_the_turn_relay_when_no_direct_path_exists() {
    let sys = TestSystem::start_with_turn().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let media = Media::open(&api(&sys, &sys.admin_token), dev, true).await.expect("media channel via TURN");
    assert_eq!(local_candidate_type(media.peer()).await, RTCIceCandidateType::Relay);
    let body = json!({ "actions": [], "observe": "clip", "media_session": media.session });
    let (status, mut obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(obs["clip"]["p2p"].is_u64(), "{obs}");
    media.fill(&mut obs).await.unwrap();

    use base64::Engine;
    let mp4 = base64::engine::general_purpose::STANDARD.decode(obs["clip"]["data"].as_str().unwrap()).unwrap();
    let size = mp4.len() as u64;
    let reader = mp4::Mp4Reader::read_header(std::io::Cursor::new(mp4), size).unwrap();
    let track = reader.tracks().values().next().unwrap();
    assert_eq!(track.media_type().unwrap(), mp4::MediaType::H264);
    assert_eq!((track.width(), track.height()), (1280, 720));
    assert!(obs["clip"].get("p2p").is_none());
    media.close().await;
}

#[tokio::test]
async fn without_a_peer_to_peer_path_the_server_relays_the_media() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    // No TURN configured: a relay-only channel can never connect.
    assert!(Media::open(&api(&sys, &sys.admin_token), dev, true).await.is_err());
    // A channel the Agent does not have (closed, never opened): the bytes stay in the reply.
    let body = json!({ "actions": [], "media_session": "gone" });
    let (status, obs) = sys.api.post_json(&format!("/api/v1/devices/{dev}/actions"), &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert!(obs["images"][0]["data"].is_string() && obs["images"][0].get("p2p").is_none(), "{obs}");
}

#[tokio::test]
async fn rcli_prints_the_same_observation_as_the_server_relay() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;

    let printed: serde_json::Value = serde_json::from_str(&sys.rcli(&["shot", dev, "--json"]).await.unwrap()).unwrap();
    let bob = sys.create_user("bob", "operator").await;
    let (_, relayed) = sys.api.with_token(Some(&bob)).post_json(&format!("/api/v1/devices/{dev}/actions"), &json!({ "actions": [] })).await;
    assert!(relayed["images"][0]["data"].is_string(), "{relayed}");
    assert_eq!(printed, relayed);
}
