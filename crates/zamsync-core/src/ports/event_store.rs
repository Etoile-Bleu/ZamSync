use crate::{Event, SequenceNumber, ZamResult};

pub trait EventStore {
    fn next_seq(&self) -> SequenceNumber;
    fn append(&mut self, event: &Event) -> ZamResult<SequenceNumber>;
    fn scan(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>>;
    fn sync(&mut self) -> ZamResult<()>;
}
