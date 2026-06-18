use crate::{Event, SequenceNumber, ZamResult};

pub trait EventStore {
    fn next_seq(&self) -> SequenceNumber;
    fn append(&mut self, event: &Event) -> ZamResult<SequenceNumber>;
    fn scan(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>>;
    fn sync(&mut self) -> ZamResult<()>;
    /// Current on-disk byte size of the backing store. Used for Prometheus metrics.
    /// Returns 0 for in-memory or non-file-backed implementations.
    fn byte_size(&self) -> u64 {
        0
    }
}
