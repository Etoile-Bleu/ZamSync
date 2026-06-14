use crate::protocol;
use std::io::BufWriter;
use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, ZamError, ZamResult};

use super::transport::TlsStream;

/// A single-connection TLS transport returned by [`super::TlsTcpTransport::accept_split`].
///
/// Owns exactly one TLS stream and implements [`Transport`] for that peer.
/// `Send`-safe (both rustls `StreamOwned` variants are `Send`): move it into
/// a worker thread so the hub can serve N TLS peers concurrently.
pub struct TlsPeerTransport {
    peer_id: NodeId,
    stream: TlsStream,
    frame_buf: protocol::FrameBuffer,
    pending: Option<SyncMessage>,
}

impl TlsPeerTransport {
    pub(super) fn new(peer_id: NodeId, stream: TlsStream, pending: Option<SyncMessage>) -> Self {
        Self {
            peer_id,
            stream,
            frame_buf: protocol::FrameBuffer::new(),
            pending,
        }
    }

    /// NodeId extracted from the peer's opening Handshake.
    pub fn peer_id(&self) -> NodeId {
        self.peer_id
    }
}

impl Transport for TlsPeerTransport {
    fn send(&mut self, _peer_id: NodeId, message: &SyncMessage) -> ZamResult<()> {
        let mut writer = BufWriter::new(&mut self.stream);
        protocol::encode(message, &mut writer)
    }

    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>> {
        if let Some(msg) = self.pending.take() {
            return Ok(Some((self.peer_id, msg)));
        }
        match self.frame_buf.try_read_frame(&mut self.stream) {
            Ok(Some(bytes)) => {
                let msg = rkyv::from_bytes::<SyncMessage>(&bytes)
                    .map_err(|e| ZamError::Serialization(format!("{}", e)))?;
                Ok(Some((self.peer_id, msg)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
