//! Controller side of a live view (what the Dashboard does in the browser), and an in-process TURN relay
//! that checks credentials like coturn's `use-auth-secret`.

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use openh264::{decoder::Decoder, formats::YUVSource};
use serde_json::{json, Value};
use webrtc::{
    api::{interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder},
    data_channel::{data_channel_init::RTCDataChannelInit, data_channel_state::RTCDataChannelState, RTCDataChannel},
    ice_transport::{ice_candidate_type::RTCIceCandidateType, ice_server::RTCIceServer},
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, policy::ice_transport_policy::RTCIceTransportPolicy,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::RTPCodecType, rtp_transceiver_direction::RTCRtpTransceiverDirection, RTCRtpTransceiverInit,
    },
    rtp::{codecs::h264::H264Packet, packetizer::Depacketizer},
    stats::StatsReportType,
    turn,
};

use crate::{eventually, ApiClient, StatusCode};

/// A viewer's peer connection receiving a Device's live stream.
pub struct LiveViewer {
    pc: Arc<RTCPeerConnection>,
    /// Live stream id from the Server, for `DELETE .../live/{session}`.
    pub session: String,
    path: String,
    packets: Arc<AtomicUsize>,
    codec: Arc<Mutex<String>>,
    /// Every complete video frame received: when, and its encoded size in bytes.
    frames: Arc<Mutex<Vec<(Instant, usize)>>>,
    /// Size of the last decoded picture.
    size: Arc<Mutex<Option<(u32, u32)>>>,
    /// The channel the Dashboard sends its Live View Quality on.
    quality: Arc<RTCDataChannel>,
}

/// Frame rate and average encoded frame size over a window of a live stream.
#[derive(Debug, Clone, Copy)]
pub struct StreamRate {
    pub fps: f64,
    pub bytes_per_frame: f64,
}

impl LiveViewer {
    /// Opens a live view of `device` through the Controller API. `relay_only` forces media through TURN.
    /// `Err` = the API refused (status and error body).
    pub async fn open(api: &ApiClient, device: &str, relay_only: bool) -> Result<Self, (StatusCode, Value)> {
        Self::connect(api, device, relay_only, None).await
    }

    /// Opens a live view of one `display` of `device`.
    pub async fn open_display(api: &ApiClient, device: &str, display: u32) -> Result<Self, (StatusCode, Value)> {
        Self::connect(api, device, false, Some(display)).await
    }

    async fn connect(api: &ApiClient, device: &str, relay_only: bool, display: Option<u32>) -> Result<Self, (StatusCode, Value)> {
        let (status, servers) = api.get("/api/v1/ice-servers").await;
        if status != StatusCode::OK {
            return Err((status, servers));
        }
        let ice_servers = servers
            .as_array()
            .unwrap()
            .iter()
            .map(|s| RTCIceServer {
                urls: serde_json::from_value(s["urls"].clone()).unwrap(),
                username: s["username"].as_str().unwrap_or_default().into(),
                credential: s["credential"].as_str().unwrap_or_default().into(),
            })
            .collect();
        let mut media = MediaEngine::default();
        media.register_default_codecs().unwrap();
        let interceptors = register_default_interceptors(Registry::new(), &mut media).unwrap();
        let api_rtc = APIBuilder::new().with_media_engine(media).with_interceptor_registry(interceptors).build();
        let policy = if relay_only { RTCIceTransportPolicy::Relay } else { RTCIceTransportPolicy::All };
        let config = RTCConfiguration { ice_servers, ice_transport_policy: policy, ..Default::default() };
        let pc = Arc::new(api_rtc.new_peer_connection(config).await.unwrap());
        let recvonly = RTCRtpTransceiverInit { direction: RTCRtpTransceiverDirection::Recvonly, send_encodings: vec![] };
        pc.add_transceiver_from_kind(RTPCodecType::Video, Some(recvonly)).await.unwrap();
        let init = RTCDataChannelInit { negotiated: Some(0), ..Default::default() };
        let quality = pc.create_data_channel("quality", Some(init)).await.unwrap();
        let (packets, codec) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(String::new())));
        let (frames, size) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(None)));
        let (p, c, f, sz) = (packets.clone(), codec.clone(), frames.clone(), size.clone());
        pc.on_track(Box::new(move |track, _, _| {
            *c.lock().unwrap() = track.codec().capability.mime_type.clone();
            let (p, f, sz) = (p.clone(), f.clone(), sz.clone());
            tokio::spawn(async move {
                let (mut depacketizer, mut decoder, mut frame) = (H264Packet::default(), Decoder::new().unwrap(), vec![]);
                while let Ok((packet, _)) = track.read_rtp().await {
                    p.fetch_add(1, Ordering::Relaxed);
                    if let Ok(nals) = depacketizer.depacketize(&packet.payload) {
                        frame.extend_from_slice(&nals);
                    }
                    if packet.header.marker {
                        f.lock().unwrap().push((Instant::now(), frame.len()));
                        if let Ok(Some(picture)) = decoder.decode(&frame) {
                            let (w, h) = picture.dimensions();
                            *sz.lock().unwrap() = Some((w as u32, h as u32));
                        }
                        frame.clear();
                    }
                }
            });
            Box::pin(async {})
        }));
        let offer = pc.create_offer(None).await.unwrap();
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.unwrap();
        let _ = gathered.recv().await;
        let sdp = pc.local_description().await.unwrap().sdp;
        let path = format!("/api/v1/devices/{device}/live");
        let (status, body) = api.post_json(&path, &json!({ "sdp": sdp, "display": display })).await;
        if status != StatusCode::CREATED {
            pc.close().await.unwrap();
            return Err((status, body));
        }
        let answer = RTCSessionDescription::answer(body["sdp"].as_str().unwrap().into()).unwrap();
        pc.set_remote_description(answer).await.unwrap();
        let session = body["session"].as_str().unwrap().to_owned();
        Ok(Self { pc, path: format!("{path}/{session}"), session, packets, codec, frames, size, quality })
    }

    /// Size in pixels of the pictures being streamed, once one has been decoded.
    pub async fn frame_size(&self) -> (u32, u32) {
        eventually(|| async { *self.size.lock().unwrap() }).await
    }

    /// Picks a Live View Quality ("quality", "balanced" or "speed") on the open stream, as the Dashboard's picker does.
    pub async fn set_quality(&self, level: &str) {
        eventually(|| async { (self.quality.ready_state() == RTCDataChannelState::Open).then_some(()) }).await;
        self.quality.send_text(level.to_owned()).await.unwrap();
    }

    /// Frame rate and average encoded frame size of what arrives during the next `window`.
    pub async fn measure(&self, window: Duration) -> StreamRate {
        let from = Instant::now();
        tokio::time::sleep(window).await;
        let frames = self.frames.lock().unwrap();
        let sizes: Vec<usize> = frames.iter().filter(|(at, _)| *at >= from).map(|(_, bytes)| *bytes).collect();
        let n = sizes.len().max(1) as f64;
        StreamRate { fps: sizes.len() as f64 / window.as_secs_f64(), bytes_per_frame: sizes.iter().sum::<usize>() as f64 / n }
    }

    /// Waits until at least `n` RTP packets of video have arrived.
    pub async fn wait_packets(&self, n: usize) {
        eventually(|| async { (self.packets.load(Ordering::Relaxed) >= n).then_some(()) }).await
    }

    pub fn codec(&self) -> String {
        self.codec.lock().unwrap().clone()
    }

    /// Candidate type ("host", "relay", ...) of this side of the selected candidate pair.
    pub async fn local_candidate_type(&self) -> RTCIceCandidateType {
        local_candidate_type(&self.pc).await
    }

    /// Stops watching: tells the Server, then closes the peer connection.
    pub async fn close(self, api: &ApiClient) -> StatusCode {
        let (status, _) = api.delete(&self.path).await;
        self.pc.close().await.unwrap();
        status
    }
}

/// Candidate type ("host", "relay", ...) of `pc`'s side of its selected candidate pair.
pub async fn local_candidate_type(pc: &RTCPeerConnection) -> RTCIceCandidateType {
    let stats = pc.get_stats().await.reports;
    let pair = stats
        .values()
        .find_map(|s| match s {
            StatsReportType::CandidatePair(p) if p.nominated => Some(p.local_candidate_id.clone()),
            _ => None,
        })
        .expect("a nominated candidate pair");
    match &stats[&pair] {
        StatsReportType::LocalCandidate(c) => c.candidate_type.into(),
        other => panic!("not a local candidate: {other:?}"),
    }
}

/// TURN relay on 127.0.0.1 accepting coturn REST API credentials made with `secret`. Dropping it stops it.
pub struct TestTurn {
    pub addr: SocketAddr,
    server: Option<turn::server::Server>,
}

impl TestTurn {
    pub async fn start(secret: &str) -> Self {
        let conn = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = conn.local_addr().unwrap();
        let server = turn::server::Server::new(turn::server::config::ServerConfig {
            conn_configs: vec![turn::server::config::ConnConfig {
                conn,
                relay_addr_generator: Box::new(turn::relay::relay_static::RelayAddressGeneratorStatic {
                    relay_address: addr.ip(),
                    address: addr.ip().to_string(),
                    net: Arc::new(webrtc::util::vnet::net::Net::new(None)),
                }),
            }],
            realm: "rcli".into(),
            auth_handler: Arc::new(turn::auth::LongTermAuthHandler::new(secret.into())),
            channel_bind_timeout: std::time::Duration::from_secs(0),
            alloc_close_notify: None,
        })
        .await
        .unwrap();
        Self { addr, server: Some(server) }
    }
}

impl Drop for TestTurn {
    fn drop(&mut self) {
        if let Some(s) = self.server.take() {
            tokio::spawn(async move { s.close().await });
        }
    }
}
