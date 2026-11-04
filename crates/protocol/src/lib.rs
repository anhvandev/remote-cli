//! Messages shared by Server and Agent, plus the framing used on the Agent connection.

pub mod agent {
    include!(concat!(env!("OUT_DIR"), "/rcli.agent.v1.rs"));
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
