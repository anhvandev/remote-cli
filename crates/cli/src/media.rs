//! Controller side of a media channel: a WebRTC data channel to the Device's Agent (peer-to-peer, TURN when
//! NAT blocks that) that brings the bytes of large Observations and clips. See the Agent's `media.rs`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::Engine;
use reqwest::Method;
use serde_json::{json, Value};
use webrtc::{
    api::APIBuilder,
    data_channel::{data_channel_init::RTCDataChannelInit, data_channel_state::RTCDataChannelState},
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{
        configuration::RTCConfiguration, policy::ice_transport_policy::RTCIceTransportPolicy,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
};

use crate::{path_segment, Api};

/// Longer than this to connect and the Server relays the media instead.
const CONNECT_WITHIN: Duration = Duration::from_secs(5);
/// A blob the Agent sent has normally arrived before its ActResult; this covers a slow path.
const BLOB_WITHIN: Duration = Duration::from_secs(10);

/// Blob id -> (total length, bytes so far).
type Blobs = Arc<Mutex<HashMap<u64, (usize, Vec<u8>)>>>;

pub struct Media {
    api: Api,
    pc: Arc<RTCPeerConnection>,
    /// Pass as `media_session` in `POST .../actions`.
    pub session: String,
    path: String,
    blobs: Blobs,
}

impl Media {
    /// Opens a media channel to `device` and waits until it is usable. `relay_only` forces TURN.
    pub async fn open(api: &Api, device: &str, relay_only: bool) -> anyhow::Result<Self> {
        let servers = api.call(Method::GET, "/api/v1/ice-servers", None).await?;
        let ice_servers = servers
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| RTCIceServer {
                urls: serde_json::from_value(s["urls"].clone()).unwrap_or_default(),
                username: s["username"].as_str().unwrap_or_default().into(),
                credential: s["credential"].as_str().unwrap_or_default().into(),
            })
            .collect();
        let policy = if relay_only { RTCIceTransportPolicy::Relay } else { RTCIceTransportPolicy::All };
        let config = RTCConfiguration { ice_servers, ice_transport_policy: policy, ..Default::default() };
        let pc = Arc::new(APIBuilder::new().build().new_peer_connection(config).await?);
        let init = RTCDataChannelInit { negotiated: Some(0), ..Default::default() };
        let dc = pc.create_data_channel("media", Some(init)).await?;
        let blobs = Blobs::default();
        let into = blobs.clone();
        dc.on_message(Box::new(move |msg| {
            if let Some((head, chunk)) = msg.data.split_at_checked(12) {
                let id = u64::from_be_bytes(head[..8].try_into().unwrap());
                let total = u32::from_be_bytes(head[8..].try_into().unwrap()) as usize;
                into.lock().unwrap().entry(id).or_insert_with(|| (total, Vec::with_capacity(total))).1.extend_from_slice(chunk);
            }
            Box::pin(async {})
        }));
        let offer = pc.create_offer(None).await?;
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await?;
        let _ = gathered.recv().await;
        let sdp = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();
        let path = format!("/api/v1/devices/{}/media", path_segment(device));
        let answer = match api.call(Method::POST, &path, Some(&json!({ "sdp": sdp }))).await {
            Ok(a) => a,
            Err(e) => {
                let _ = pc.close().await;
                return Err(e);
            }
        };
        let session = answer["session"].as_str().unwrap_or_default().to_owned();
        let media = Self { api: api.clone(), pc, path: format!("{path}/{session}"), session, blobs };
        let opened = async {
            media.pc.set_remote_description(RTCSessionDescription::answer(answer["sdp"].as_str().unwrap_or_default().into())?).await?;
            let started = Instant::now();
            while dc.ready_state() != RTCDataChannelState::Open {
                anyhow::ensure!(started.elapsed() < CONNECT_WITHIN, "media channel did not connect");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok(())
        };
        match opened.await {
            Ok(()) => Ok(media),
            Err(e) => {
                media.close().await;
                Err(e)
            }
        }
    }

    /// The peer connection (e.g. to see which candidate pair carries the media).
    pub fn peer(&self) -> &RTCPeerConnection {
        &self.pc
    }

    /// Puts the bytes of every `p2p` image/clip of an Observation back into its `data` (base64), so it reads
    /// exactly as if the Server had relayed them.
    pub async fn fill(&self, obs: &mut Value) -> anyhow::Result<()> {
        for image in obs["images"].as_array_mut().into_iter().flatten() {
            self.fill_one(image).await?;
        }
        if let Some(clip) = obs.get_mut("clip") {
            self.fill_one(clip).await?;
        }
        Ok(())
    }

    async fn fill_one(&self, item: &mut Value) -> anyhow::Result<()> {
        if let Some(id) = item.as_object_mut().and_then(|o| o.remove("p2p")).and_then(|v| v.as_u64()) {
            item["data"] = base64::engine::general_purpose::STANDARD.encode(self.take(id).await?).into();
        }
        Ok(())
    }

    async fn take(&self, id: u64) -> anyhow::Result<Vec<u8>> {
        let started = Instant::now();
        loop {
            {
                let mut blobs = self.blobs.lock().unwrap();
                if blobs.get(&id).is_some_and(|(total, data)| data.len() >= *total) {
                    return Ok(blobs.remove(&id).unwrap().1);
                }
            }
            // shortcut: the Actions already ran, so a lost blob is an error, not a retry; add a re-send if it shows up.
            anyhow::ensure!(started.elapsed() < BLOB_WITHIN, "media channel lost an Observation");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Tells the Server (and so the Agent) and closes the peer connection.
    pub async fn close(self) {
        let _ = self.api.call(Method::DELETE, &self.path, None).await;
        let _ = self.pc.close().await;
    }
}

impl Api {
    /// `POST .../actions` with `body`. Unless nothing is observed, large media comes over a fresh media channel;
    /// if one cannot be opened the Server relays it. The result is the same either way.
    pub async fn observe(&self, device: &str, body: &Value) -> anyhow::Result<Value> {
        let path = format!("/api/v1/devices/{device}/actions");
        let media = match body["observe"] == "none" {
            true => None,
            false => Media::open(self, device, false).await.ok(),
        };
        let Some(media) = media else { return self.call(Method::POST, &path, Some(body)).await };
        let mut body = body.clone();
        body["media_session"] = media.session.clone().into();
        let mut result = self.call(Method::POST, &path, Some(&body)).await;
        if let Ok(obs) = &mut result {
            if let Err(e) = media.fill(obs).await {
                result = Err(e);
            }
        }
        media.close().await;
        result
    }
}
