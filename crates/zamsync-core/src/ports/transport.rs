use crate::{NodeId, SyncMessage, ZamResult};

pub trait Transport {
    fn send(&mut self, peer_id: NodeId, message: &SyncMessage) -> ZamResult<()>;
    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>>;
}
