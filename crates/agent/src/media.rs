//! Media channels: a Controller's peer connection with one data channel that carries the bytes of large
//! Observations and clips, so they skip the Server. Signaling is the live view's (`LiveOffer` with `media`).
//!
//! Wire format of the data channel (Agent -> Controller only): a blob is sent as messages of
//! `[u64 id BE][u32 total length BE][next bytes]`; the ActResult names the blob by `id` (`p2p`).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use rcli_protocol::agent::{ActResult, Error, LiveAnswer, LiveOffer};
use webrtc::{
    data_channel::{data_channel_init::RTCDataChannelInit, data_channel_state::RTCDataChannelState, RTCDataChannel},
    peer_connection::{
        peer_connection_state::RTCPeerConnectionState, sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
};

/// Smaller images stay in the ActResult: a separate message would not save anything.
pub const P2P_MIN_BYTES: usize = 4096;
/// One data channel message, header included (webrtc-rs reads at most 16 KiB per message).
const MESSAGE_BYTES: usize = 16 * 1024;
const HEADER_BYTES: usize = 12;
/// A Controller that has not connected by then is gone.
const CONNECT_WITHIN: Duration = Duration::from_secs(30);
/// How long an act result waits for a channel that is still opening before using the Server instead.
const OPEN_WAIT: Duration = Duration::from_secs(1);

struct Channel {
    pc: Arc<RTCPeerConnection>,
    dc: Arc<RTCDataChannel>,
}

/// Open media channels by session id. Outlives Server connections like live streams.
#[derive(Default)]
pub struct Media {
    channels: Mutex<HashMap<String, Channel>>,
    next_blob: AtomicU64,
}

impl Media {
    /// Answers a Controller's offer. Its data channel is pre-negotiated (id 0), so it exists on both sides at once.
    pub async fn open(self: &Arc<Self>, offer: LiveOffer) -> LiveAnswer {
        let request_id = offer.request_id;
        match self.start(offer).await {
            Ok(sdp) => LiveAnswer { request_id, sdp, error: None },
            Err(e) => LiveAnswer { request_id, sdp: String::new(), error: Some(e) },
        }
    }

    pub async fn close(&self, session: &str) {
        let channel = self.channels.lock().unwrap().remove(session);
        if let Some(c) = channel {
            let _ = c.pc.close().await;
        }
    }

    /// Closes every media channel (e.g. when the person at the Device disconnects a Controller).
    pub async fn close_all(&self) {
        let channels: Vec<Channel> = self.channels.lock().unwrap().drain().map(|(_, c)| c).collect();
        for c in channels {
            let _ = c.pc.close().await;
        }
    }

    async fn start(self: &Arc<Self>, offer: LiveOffer) -> Result<String, Error> {
        fn fail(code: &'static str) -> impl Fn(webrtc::Error) -> Error {
            move |e| Error { code: code.into(), message: e.to_string() }
        }
        let remote = RTCSessionDescription::offer(offer.sdp).map_err(fail("invalid_offer"))?;
        let pc = Arc::new(crate::live::peer_connection(&offer.ice_servers).await.map_err(fail("media"))?);
        let init = RTCDataChannelInit { negotiated: Some(0), ..Default::default() };
        let dc = pc.create_data_channel("media", Some(init)).await.map_err(fail("media"))?;
        if let Err(e) = pc.set_remote_description(remote).await {
            let _ = pc.close().await;
            return Err(fail("invalid_offer")(e));
        }
        let answer = pc.create_answer(None).await.map_err(fail("media"))?;
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(answer).await.map_err(fail("media"))?;
        let _ = gathered.recv().await;
        let sdp = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();
        self.channels.lock().unwrap().insert(offer.session.clone(), Channel { pc: pc.clone(), dc });

        let (me, session) = (Arc::downgrade(self), offer.session);
        pc.on_peer_connection_state_change(Box::new({
            let (me, session) = (me.clone(), session.clone());
            move |state| {
                let (me, session) = (me.clone(), session.clone());
                if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                    if let Some(me) = Weak::upgrade(&me) {
                        // Not inline: closing from inside this handler would re-enter it.
                        tokio::spawn(async move { me.close(&session).await });
                    }
                }
                Box::pin(async {})
            }
        }));
        tokio::spawn(async move {
            tokio::time::sleep(CONNECT_WITHIN).await;
            if let Some(me) = me.upgrade() {
                let idle = me.channels.lock().unwrap().get(&session).is_some_and(|c| c.pc.connection_state() != RTCPeerConnectionState::Connected);
                if idle {
                    me.close(&session).await;
                }
            }
        });
        Ok(sdp)
    }

    /// Moves the clip and large image bytes of `result` onto media channel `session` (marking them `p2p`).
    /// Anything that cannot go that way (no such channel, not open, send failed) stays in `result`.
    pub async fn offload(&self, session: &str, result: &mut ActResult) {
        let Some(dc) = self.channels.lock().unwrap().get(session).map(|c| c.dc.clone()) else { return };
        let started = Instant::now();
        while dc.ready_state() == RTCDataChannelState::Connecting && started.elapsed() < OPEN_WAIT {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if dc.ready_state() != RTCDataChannelState::Open {
            return;
        }
        // Clips always; images only when large.
        let images = result.images.iter_mut().filter(|i| i.data.len() >= P2P_MIN_BYTES).map(|i| (&mut i.data, &mut i.p2p));
        for (data, p2p) in images.chain(result.clip.as_mut().map(|c| (&mut c.data, &mut c.p2p))) {
            let id = self.next_blob.fetch_add(1, Ordering::Relaxed) + 1;
            if send(&dc, id, data).await.is_ok() {
                data.clear();
                *p2p = id;
            }
        }
    }
}

async fn send(dc: &RTCDataChannel, id: u64, data: &[u8]) -> webrtc::error::Result<()> {
    let total = u32::try_from(data.len()).expect("Observations are far below 4 GiB");
    for chunk in data.chunks(MESSAGE_BYTES - HEADER_BYTES) {
        let mut msg = Vec::with_capacity(HEADER_BYTES + chunk.len());
        msg.extend_from_slice(&id.to_be_bytes());
        msg.extend_from_slice(&total.to_be_bytes());
        msg.extend_from_slice(chunk);
        dc.send(&Bytes::from(msg)).await?;
    }
    Ok(())
}
