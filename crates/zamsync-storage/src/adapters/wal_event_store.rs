use crate::encryption::EncryptionKey;
use crate::wal::{WalScanner, WalWriter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zamsync_core::ports::EventStore;
use zamsync_core::{Event, SequenceNumber, ZamError, ZamResult};

pub struct WalEventStore {
    path: PathBuf,
    writer: WalWriter,
    encryption: Option<Arc<EncryptionKey>>,
}

impl WalEventStore {
    pub fn open(path: impl AsRef<Path>) -> ZamResult<Self> {
        Self::open_inner(path, None)
    }

    pub fn open_encrypted(path: impl AsRef<Path>, key: EncryptionKey) -> ZamResult<Self> {
        Self::open_inner(path, Some(Arc::new(key)))
    }

    fn open_inner(path: impl AsRef<Path>, encryption: Option<Arc<EncryptionKey>>) -> ZamResult<Self> {
        let (last_seq, end_pos) = match &encryption {
            Some(key) => WalScanner::recover_encrypted(&path, Arc::clone(key))?,
            None => WalScanner::recover(&path)?,
        };

        if path.as_ref().exists() {
            let actual_len = std::fs::metadata(path.as_ref())?.len();
            if actual_len > end_pos {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path.as_ref())?
                    .set_len(end_pos)?;
            }
        }

        let next_seq = last_seq.map(|s| s.next()).unwrap_or(SequenceNumber::ZERO);
        let writer = match &encryption {
            Some(key) => WalWriter::open_encrypted(&path, next_seq, Arc::clone(key))?,
            None => WalWriter::open(&path, next_seq)?,
        };

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            writer,
            encryption,
        })
    }
}

impl WalEventStore {
    pub fn compact(&mut self, frontier: &HashMap<u32, SequenceNumber>) -> ZamResult<usize> {
        if !self.path.exists() || frontier.is_empty() {
            return Ok(0);
        }

        self.writer.sync()?;

        let mut kept: Vec<(SequenceNumber, Vec<u8>)> = Vec::new();
        let mut dropped = 0usize;
        let mut last_seen_seq: Option<SequenceNumber> = None;

        let scanner = match &self.encryption {
            Some(key) => WalScanner::open_encrypted(&self.path, Arc::clone(key))?,
            None => WalScanner::open(&self.path)?,
        };

        for result in scanner.scan() {
            let record = result?;
            last_seen_seq = Some(record.seq);

            if record.payload.is_empty() {
                kept.push((record.seq, record.payload));
                continue;
            }

            let event: Event = rkyv::from_bytes(&record.payload)
                .map_err(|e| ZamError::Serialization(format!("{}", e)))?;

            let below_frontier = frontier
                .get(&event.origin_node.0)
                .map(|&frontier_seq| event.seq <= frontier_seq)
                .unwrap_or(false);

            if below_frontier {
                dropped += 1;
            } else {
                kept.push((record.seq, record.payload));
            }
        }

        if dropped == 0 {
            return Ok(0);
        }

        let all_tombstones = kept.iter().all(|(_, p)| p.is_empty());
        if all_tombstones {
            kept.clear();
            if let Some(last_seq) = last_seen_seq {
                kept.push((last_seq, Vec::new()));
            }
        }

        let tmp = self.path.with_extension("wal.tmp");
        {
            let mut w = match &self.encryption {
                Some(key) => WalWriter::open_encrypted(&tmp, SequenceNumber::ZERO, Arc::clone(key))?,
                None => WalWriter::open(&tmp, SequenceNumber::ZERO)?,
            };
            for (seq, payload) in &kept {
                w.append_at_seq(*seq, payload)?;
            }
            w.sync()?;
        }

        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        std::fs::rename(&tmp, &self.path)?;

        let (last_seq, end_pos) = match &self.encryption {
            Some(key) => WalScanner::recover_encrypted(&self.path, Arc::clone(key))?,
            None => WalScanner::recover(&self.path)?,
        };
        let next_seq = last_seq.map(|s| s.next()).unwrap_or(SequenceNumber::ZERO);

        let actual_len = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if actual_len > end_pos {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&self.path)?
                .set_len(end_pos)?;
        }

        self.writer = match &self.encryption {
            Some(key) => WalWriter::open_encrypted(&self.path, next_seq, Arc::clone(key))?,
            None => WalWriter::open(&self.path, next_seq)?,
        };
        Ok(dropped)
    }
}

impl EventStore for WalEventStore {
    fn next_seq(&self) -> SequenceNumber {
        self.writer.next_seq()
    }

    fn append(&mut self, event: &Event) -> ZamResult<SequenceNumber> {
        let bytes =
            rkyv::to_bytes::<_, 1024>(event).map_err(|e| ZamError::Serialization(e.to_string()))?;
        self.writer.append(&bytes)
    }

    fn scan(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>> {
        if !self.path.exists() {
            return Ok(Box::new(std::iter::empty()));
        }
        let scanner = match &self.encryption {
            Some(key) => WalScanner::open_encrypted(&self.path, Arc::clone(key))?,
            None => WalScanner::open(&self.path)?,
        };
        let iter = scanner.scan().filter_map(|res| -> Option<ZamResult<Event>> {
            let record = match res {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            if record.payload.is_empty() {
                return None;
            }
            Some(
                rkyv::from_bytes::<Event>(&record.payload)
                    .map_err(|e| ZamError::Serialization(format!("{}", e))),
            )
        });
        Ok(Box::new(iter))
    }

    fn sync(&mut self) -> ZamResult<()> {
        self.writer.sync().map_err(Into::into)
    }
}
