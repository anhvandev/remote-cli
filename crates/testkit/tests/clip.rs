use std::{io::Cursor, time::Duration};

use rcli_testkit::{StatusCode, TestSystem};
use serde_json::{json, Value};

/// Duration and sample count of the MP4's H.264 track.
fn h264_track(clip: &Value) -> (Duration, u32) {
    use base64::Engine;
    let mp4 = base64::engine::general_purpose::STANDARD.decode(clip["data"].as_str().unwrap()).unwrap();
    let size = mp4.len() as u64;
    let reader = mp4::Mp4Reader::read_header(Cursor::new(mp4), size).unwrap();
    let track = reader.tracks().values().next().unwrap();
    assert_eq!(track.media_type().unwrap(), mp4::MediaType::H264);
    assert_eq!((track.width() as u64, track.height() as u64), (clip["width"].as_u64().unwrap(), clip["height"].as_u64().unwrap()));
    (track.duration(), track.sample_count())
}

#[tokio::test]
async fn clip_records_the_action_until_the_screen_settles() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    // The screen keeps changing for 1 s after the click.
    agent.platform.animate_after_input(Duration::from_secs(1));
    let body = json!({ "actions": [{"type": "click", "x": 1, "y": 1}], "observe": "clip" });
    let (status, obs) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    assert_eq!(obs["images"], json!([]));
    let clip = &obs["clip"];
    assert_eq!((clip["display"].clone(), clip["mime"].clone()), (json!(0), json!("video/mp4")), "{clip}");
    assert_eq!((clip["width"].clone(), clip["height"].clone()), (json!(1280), json!(720)));
    let (duration, frames) = h264_track(clip);
    assert!(duration >= Duration::from_secs(1) && duration < Duration::from_secs(3), "{duration:?}");
    assert!(frames >= 5, "{frames} frames");
}

#[tokio::test]
async fn clip_length_is_capped_and_covers_one_display() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await;
    let actions = format!("/api/v1/devices/{dev}/actions");

    // A screen that never settles.
    agent.platform.animate_after_input(Duration::from_secs(60));
    let body = json!({ "actions": [{"type": "key", "keys": "a"}], "observe": "clip" });
    let (status, obs) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::OK, "{obs}");
    let (duration, _) = h264_track(&obs["clip"]);
    assert!(duration >= Duration::from_secs(5) && duration <= Duration::from_millis(5200), "{duration:?}");
    assert_eq!(obs["clip"]["duration_ms"].as_u64().unwrap() as u128, duration.as_millis());

    let body = json!({ "actions": [], "observe": "clip", "display": "all" });
    let (status, obs) = sys.api.post_json(&actions, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{obs}");
    assert_eq!(obs["error"]["code"], "invalid_action");
}

#[tokio::test]
async fn cli_saves_the_clip() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let dev = agent.device_id();
    sys.wait_device(dev, |d| d["status"] == "Online").await;
    sys.rcli(&["lock", "acquire", dev]).await.unwrap();
    let dir = std::env::temp_dir().join(format!("rcli-clip-{dev}"));
    let out = sys.rcli(&["act", dev, "click", "1", "1", "--observe", "clip", "--out", dir.to_str().unwrap()]).await.unwrap();
    assert!(out.contains("clip.mp4"), "{out}");
    let mp4 = std::fs::read(dir.join("clip.mp4")).unwrap();
    assert_eq!(&mp4[4..8], b"ftyp");
    let _ = std::fs::remove_dir_all(dir);
}
