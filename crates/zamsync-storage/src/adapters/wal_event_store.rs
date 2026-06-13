use std::path::{Path, PathBuf};
use zamsync_core::{Event, SequenceNumber, ZamError, ZamResult};
use zamsync_core::ports::EventStore;
use crate::wal::{WalScanner, WalWriter};

pub struct WalEventStore {
    path: PathBuf,
    writer: WalWriter,
}

impl WalEventStore {
    pub fn open(path: impl AsRef<Path>) -> ZamResult<Self> {
        let (last_seq, _) = WalScanner::recover(&path)?;
        let next_seq = last_seq.map(|s| s.next()).unwrap_or(SequenceNumber::ZERO);
        let writer = WalWriter::open(&path, next_seq)?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            writer,
        })
    }
}

impl EventStore for WalEventStore {
    fn next_seq(&self) -> SequenceNumber {
        self.writer.next_seq()
    }

    fn append(&mut self, event: &Event) -> ZamResult<SequenceNumber> {
        let bytes = rkyv::to_bytes::<_, 1024>(event)
            .map_err(|e| ZamError::Serialization(e.to_string()))?;
        self.writer.append(&bytes)
    }

    fn scan(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>> {
        if !self.path.exists() {
            return Ok(Box::new(std::iter::empty()));
        }
        let scanner = WalScanner::open(&self.path)?;
        let iter = scanner.scan().map(|res| -> ZamResult<Event> {
            let record = res?;
            rkyv::from_bytes::<Event>(&record.payload)
                .map_err(|e| ZamError::Serialization(format!("{}", e)))
        });
        Ok(Box::new(iter))
    }

    fn sync(&mut self) -> ZamResult<()> {
        self.writer.sync().map_err(Into::into)
    }
}
