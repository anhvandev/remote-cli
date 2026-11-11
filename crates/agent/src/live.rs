//! Live stream: the Agent's WebRTC peer. Each live view is one peer connection sending a display as H.264;
//! the control indicator shows while any is open. Signaling arrives over the Server connection.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

use image::{imageops, RgbaImage};
use rcli_protocol::agent::{Error, LiveAnswer, LiveOffer};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264},
        setting_engine::SettingEngine,
        APIBuilder,
    },
    data_channel::data_channel_init::RTCDataChannelInit,
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtcp::payload_feedbacks::{full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

use crate::{
    encoder::{self, H264},
    platform::Platform,
};

/// A viewer that has not connected by then is gone (e.g. closed the tab before ICE finished).
const CONNECT_WITHIN: Duration = Duration::from_secs(30);

/// Live View Quality: what one viewer trades between sharpness and smoothness. Every level streams native pixels;
/// they differ only in bitrate and frame rate. The viewer picks it on the stream's `quality` data channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Quality,
    Balanced,
    Speed,
}

impl Level {
    fn parse(name: &[u8]) -> Option<Self> {
        match name {
            b"quality" => Some(Self::Quality),
            b"balanced" => Some(Self::Balanced),
            b"speed" => Some(Self::Speed),
            _ => None,
        }
    }

    /// Target bitrate in bits per second and frame rate: the one place each level is tuned, for every encoder.
    fn encoding(self) -> (u32, f32) {
        match self {
            Self::Quality => (8_000_000, 10.0),
            Self::Balanced => (4_000_000, 15.0),
            Self::Speed => (2_000_000, 25.0),
        }
    }
}

struct Stream {
    pc: Arc<RTCPeerConnection>,
    stop: Arc<AtomicBool>,
}

/// Open live streams by session id, and the control indicator, which shows while the Control Lock is held
/// (the Server says so) or any live stream is open. Outlives Server connections: media is peer-to-peer.
pub struct Live {
    platform: Arc<dyn Platform>,
    streams: Mutex<HashMap<String, Stream>>,
    lock_held: AtomicBool,
}

impl Live {
    pub fn new(platform: Arc<dyn Platform>) -> Arc<Self> {
        Arc::new(Self { platform, streams: Default::default(), lock_held: AtomicBool::new(false) })
    }

    /// Answers a viewer's offer and starts streaming once connected.
    pub async fn open(self: &Arc<Self>, offer: LiveOffer) -> LiveAnswer {
        let request_id = offer.request_id;
        match self.start(offer).await {
            Ok(sdp) => LiveAnswer { request_id, sdp, error: None },
            Err(error) => LiveAnswer { request_id, sdp: String::new(), error: Some(error) },
        }
    }

    pub async fn close(&self, session: &str) {
        let stream = self.streams.lock().unwrap().remove(session);
        if let Some(s) = stream {
            s.stop.store(true, Ordering::SeqCst);
            let _ = s.pc.close().await;
        }
        self.update_indicator();
    }

    /// The Server's view of the Control Lock (`ControlIndicator`).
    pub fn set_lock_indicator(&self, on: bool) {
        self.lock_held.store(on, Ordering::SeqCst);
        self.update_indicator();
    }

    fn update_indicator(&self) {
        // Under the streams lock so concurrent updates apply in order.
        let streams = self.streams.lock().unwrap();
        let visible = self.lock_held.load(Ordering::SeqCst) || !streams.is_empty();
        if let Err(e) = self.platform.set_control_indicator(visible) {
            tracing::warn!("cannot set the control indicator: {e:#}");
        }
    }

    async fn start(self: &Arc<Self>, offer: LiveOffer) -> Result<String, Error> {
        fn fail(code: &'static str) -> impl Fn(webrtc::Error) -> Error {
            move |e| Error { code: code.into(), message: e.to_string() }
        }
        let displays = self.platform.displays().map_err(|e| Error { code: "platform".into(), message: format!("{e:#}") })?;
        if displays.is_empty() {
            return Err(Error { code: "no_display".into(), message: "Device has no display".into() });
        }
        let display = offer.display.or_else(|| displays.iter().find(|d| d.primary).map(|d| d.index)).unwrap_or(0);
        // A hardware encoder sends High profile: only to a viewer that takes it, and only when one starts here.
        let hardware = takes_high(&offer.sdp)
            && self.platform.hardware_encoder()
            && tokio::task::spawn_blocking(encoder::hardware_starts).await.unwrap_or(false);
        let remote = RTCSessionDescription::offer(offer.sdp).map_err(fail("invalid_offer"))?;
        let pc = Arc::new(peer_connection(&offer.ice_servers).await.map_err(fail("live"))?);
        // The payload type the track binds to is the offered one with this profile, so the viewer decodes it as such.
        let codec = RTCRtpCodecCapability {
            mime_type: MIME_TYPE_H264.into(),
            clock_rate: 90_000,
            sdp_fmtp_line: format!(
                "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id={}",
                encoder::profile_level_id(hardware)
            ),
            ..Default::default()
        };
        let track = Arc::new(TrackLocalStaticSample::new(codec, "screen".into(), "rcli".into()));
        let sender = pc.add_track(track.clone()).await.map_err(fail("live"))?;
        // Negotiated like the media channel: the viewer creates the same id, so no extra signaling.
        let init = RTCDataChannelInit { negotiated: Some(0), ..Default::default() };
        let picker = pc.create_data_channel("quality", Some(init)).await.map_err(fail("live"))?;
        // A new session starts at Balanced; nothing is remembered.
        let level = Arc::new(Mutex::new(Level::Balanced));
        let picked = level.clone();
        picker.on_message(Box::new(move |msg| {
            if let Some(l) = Level::parse(&msg.data) {
                *picked.lock().unwrap() = l;
            }
            Box::pin(async {})
        }));
        if let Err(e) = pc.set_remote_description(remote).await {
            let _ = pc.close().await;
            return Err(fail("invalid_offer")(e));
        }
        let answer = pc.create_answer(None).await.map_err(fail("live"))?;
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(answer).await.map_err(fail("live"))?;
        let _ = gathered.recv().await;
        let sdp = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();

        let stop = Arc::new(AtomicBool::new(false));
        let keyframe = Arc::new(AtomicBool::new(false));
        self.streams.lock().unwrap().insert(offer.session.clone(), Stream { pc: pc.clone(), stop: stop.clone() });
        self.update_indicator();

        let (me, session) = (Arc::downgrade(self), offer.session.clone());
        pc.on_peer_connection_state_change(Box::new(move |state| {
            let (me, session) = (me.clone(), session.clone());
            if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                if let Some(me) = Weak::upgrade(&me) {
                    // Not inline: closing from inside this handler would re-enter it.
                    tokio::spawn(async move { me.close(&session).await });
                }
            }
            Box::pin(async {})
        }));
        // Reading RTCP drives the interceptors; a PLI/FIR means the viewer needs a fresh keyframe.
        let wants_keyframe = keyframe.clone();
        tokio::spawn(async move {
            while let Ok((packets, _)) = sender.read_rtcp().await {
                if packets.iter().any(|p| {
                    p.as_any().downcast_ref::<PictureLossIndication>().is_some()
                        || p.as_any().downcast_ref::<FullIntraRequest>().is_some()
                }) {
                    wants_keyframe.store(true, Ordering::SeqCst);
                }
            }
        });
        let (me, session, platform) = (Arc::downgrade(self), offer.session, self.platform.clone());
        let rt = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = stream(platform.as_ref(), &session, display, hardware, &level, &pc, &track, &stop, &keyframe, &rt) {
                tracing::warn!("live stream {session} ended: {e:#}");
            }
            if let Some(me) = me.upgrade() {
                rt.block_on(me.close(&session));
            }
        });
        Ok(sdp)
    }
}

/// Whether the viewer's offer takes High profile H.264 with packetization mode 1, as Chrome does and Firefox does not.
fn takes_high(sdp: &str) -> bool {
    sdp.lines().filter(|l| l.starts_with("a=fmtp:")).map(str::to_ascii_lowercase).any(|l| {
        l.contains("profile-level-id=6400") && l.contains("packetization-mode=1")
    })
}

pub(crate) async fn peer_connection(ice_servers: &[rcli_protocol::agent::IceServer]) -> webrtc::error::Result<RTCPeerConnection> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    let interceptors = register_default_interceptors(Registry::new(), &mut media)?;
    let mut settings = SettingEngine::default();
    // Notice a vanished viewer within seconds instead of the 30 s default.
    settings.set_ice_timeouts(Some(Duration::from_secs(3)), Some(Duration::from_secs(10)), Some(Duration::from_secs(1)));
    let api = APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(interceptors)
        .with_setting_engine(settings)
        .build();
    let ice_servers = ice_servers
        .iter()
        .map(|s| RTCIceServer { urls: s.urls.clone(), username: s.username.clone(), credential: s.credential.clone() })
        .collect();
    api.new_peer_connection(RTCConfiguration { ice_servers, ..Default::default() }).await
}

/// Captures `display` at its native size and sends it as H.264 at the viewer's `level` until `stop`, on a hardware
/// encoder while `hardware` allows and one works.
#[allow(clippy::too_many_arguments)]
fn stream(
    p: &dyn Platform,
    session: &str,
    display: u32,
    mut hardware: bool,
    level: &Mutex<Level>,
    pc: &RTCPeerConnection,
    track: &TrackLocalStaticSample,
    stop: &AtomicBool,
    keyframe: &AtomicBool,
    rt: &tokio::runtime::Handle,
) -> anyhow::Result<()> {
    let started = Instant::now();
    while pc.connection_state() != RTCPeerConnectionState::Connected {
        anyhow::ensure!(started.elapsed() < CONNECT_WITHIN, "viewer did not connect");
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut current = None;
    let mut encoder: Option<H264> = None;
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let tick = Instant::now();
        let wanted = *level.lock().unwrap();
        let (bps, fps) = wanted.encoding();
        let frame = p.capture(display)?;
        // Native pixels, but H.264 4:2:0 needs even dimensions: an odd last row or column is dropped.
        let (w, h) = (frame.width & !1, frame.height & !1);
        let mut rgba = RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
            .ok_or_else(|| anyhow::anyhow!("frame size does not match its pixels"))?;
        if (w, h) != rgba.dimensions() {
            rgba = imageops::crop_imm(&rgba, 0, 0, w, h).to_image();
        }
        // A new encoder starts with a keyframe: on a level switch or a new display size.
        if current != Some(wanted) || encoder.as_ref().is_some_and(|e| e.size() != (w, h)) {
            let e = H264::open(w, h, bps, fps, hardware)?;
            tracing::info!("live stream {session}: H.264 through {} at {wanted:?}", e.name());
            encoder = Some(e);
            current = Some(wanted);
        }
        let encoder = encoder.as_mut().expect("set above");
        if keyframe.swap(false, Ordering::SeqCst) {
            encoder.force_intra_frame()?;
        }
        let data = encoder.encode(&rgba)?;
        // A hardware encoder that did not start or failed is not tried again in this stream.
        hardware &= encoder.is_hardware();
        if !data.is_empty() {
            let sample = Sample { data: data.into(), duration: last.elapsed(), ..Default::default() };
            last = Instant::now();
            rt.block_on(track.write_sample(&sample))?;
        }
        let every = Duration::from_secs_f32(1.0 / fps);
        std::thread::sleep(every.saturating_sub(tick.elapsed()));
    }
    Ok(())
}
