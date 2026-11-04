//! Messages shared by Server and Agent, plus the framing used on the Agent connection.

pub mod agent {
    include!(concat!(env!("OUT_DIR"), "/rcli.agent.v1.rs"));
}

pub mod update;

/// `Resources.session` as "Active" | "LockScreen" | "LoginScreen" | null in JSON.
fn serialize_session<S: serde::Serializer>(v: &Option<i32>, s: S) -> Result<S::Ok, S::Error> {
    use agent::SessionState::*;
    let name = match v.and_then(|v| agent::SessionState::try_from(v).ok()) {
        Some(Active) => Some("Active"),
        Some(LockScreen) => Some("LockScreen"),
        Some(LoginScreen) => Some("LoginScreen"),
        Some(Unspecified) | None => None,
    };
    s.serialize_some(&name)
}

use futures::{SinkExt, StreamExt};
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// A framed protobuf connection: sends `S`, receives `R`.
pub struct Conn<T> {
    inner: Framed<T, LengthDelimitedCodec>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> Conn<T> {
    pub fn new(io: T) -> Self {
        Self { inner: Framed::new(io, LengthDelimitedCodec::new()) }
    }

    /// Like `new` but frames up to `max` bytes (the 8 MiB default is too small for Agent builds).
    /// Use it only once the peer is authenticated: the receiver reserves up to `max` per frame.
    pub fn with_max_frame(io: T, max: usize) -> Self {
        Self { inner: Framed::new(io, LengthDelimitedCodec::builder().max_frame_length(max).new_codec()) }
    }

    pub async fn send<S: Message>(&mut self, msg: &S) -> anyhow::Result<()> {
        self.inner.send(bytes::Bytes::from(msg.encode_to_vec())).await?;
        Ok(())
    }

    /// `Ok(None)` when the peer closed the connection.
    pub async fn recv<R: Message + Default>(&mut self) -> anyhow::Result<Option<R>> {
        match self.inner.next().await {
            None => Ok(None),
            Some(frame) => Ok(Some(R::decode(frame?)?)),
        }
    }
}
