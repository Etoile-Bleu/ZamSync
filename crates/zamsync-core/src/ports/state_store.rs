use crate::{Event, SequenceNumber, ZamResult};

pub trait StateStore {
    fn apply_event(&mut self, seq: SequenceNumber, event: &Event) -> ZamResult<()>;
    fn last_applied_seq(&self) -> Option<SequenceNumber>;
}
