use crate::{NodeId, SyncMessage, ZamResult};

pub trait Transport {
    /// Encodes and sends `message` to `peer_id`. Returns the number of bytes
    /// written to the wire so callers can track bandwidth consumption.
    fn send(&mut self, peer_id: NodeId, message: &SyncMessage) -> ZamResult<usize>;
    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>>;
}
