use crate::{ReplicationState, ZamResult};

pub trait PeerStore {
    fn load(&self) -> ZamResult<ReplicationState>;
    fn save(&mut self, state: &ReplicationState) -> ZamResult<()>;
}
