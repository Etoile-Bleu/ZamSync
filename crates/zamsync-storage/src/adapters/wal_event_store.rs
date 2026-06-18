use crate::encryption::EncryptionKey;
use crate::wal::{WalScanner, WalWriter};
use metrics::{counter, gauge};
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

    fn open_inner(
        path: impl AsRef<Path>,
        encryption: Option<Arc<EncryptionKey>>,
    ) -> ZamResult<Self> {
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
                Some(key) => {
                    WalWriter::open_encrypted(&tmp, SequenceNumber::ZERO, Arc::clone(key))?
                }
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

impl WalEventStore {
    /// Remove events older than `cutoff_ms` (HLC physical timestamp in milliseconds).
    /// Always keeps the `min_keep` most recent events per origin node regardless of age,
    /// so nodes are never left completely empty.
    /// Tombstone records (empty payload) are always preserved.
    ///
    /// Returns `(dropped, bytes_freed)` where `bytes_freed` is the reduction in WAL
    /// file size in bytes. Returns `(0, 0)` when nothing qualifies for removal.
    pub fn expire_before(&mut self, cutoff_ms: u64, min_keep: usize) -> ZamResult<(usize, u64)> {
        if !self.path.exists() {
            return Ok((0, 0));
        }
        self.writer.sync()?;

        let size_before = std::fs::metadata(&self.path)?.len();

        // Collect all raw records in WAL order (oldest first).
        let mut tombstones: Vec<(SequenceNumber, Vec<u8>)> = Vec::new();
        // Per-node: list of (seq, raw_payload, hlc_physical_ms) in WAL order.
        let mut by_node: HashMap<u32, Vec<(SequenceNumber, Vec<u8>, u64)>> = HashMap::new();

        let scanner = match &self.encryption {
            Some(key) => WalScanner::open_encrypted(&self.path, Arc::clone(key))?,
            None => WalScanner::open(&self.path)?,
        };
        for result in scanner.scan() {
            let record = result?;
            if record.payload.is_empty() {
                tombstones.push((record.seq, record.payload));
                continue;
            }
            let event: Event = rkyv::from_bytes(&record.payload)
                .map_err(|e| ZamError::Serialization(format!("{e}")))?;
            by_node.entry(event.origin_node.0).or_default().push((
                record.seq,
                record.payload,
                event.hlc.physical,
            ));
        }

        // Decide what to keep: newer-than-cutoff OR within the last min_keep per node.
        let mut kept: Vec<(SequenceNumber, Vec<u8>)> = Vec::new();
        let mut dropped = 0usize;
        let mut oldest_phys_ms_kept: Option<u64> = None;

        for (_node, records) in by_node {
            let len = records.len();
            let min_keep_start = len.saturating_sub(min_keep);
            for (i, (seq, payload, phys_ms)) in records.into_iter().enumerate() {
                if phys_ms >= cutoff_ms || i >= min_keep_start {
                    oldest_phys_ms_kept =
                        Some(oldest_phys_ms_kept.map_or(phys_ms, |o: u64| o.min(phys_ms)));
                    kept.push((seq, payload));
                } else {
                    dropped += 1;
                }
            }
        }

        if dropped == 0 {
            gauge!("zamsync_wal_size_bytes").set(size_before as f64);
            if let Some(oldest_ms) = oldest_phys_ms_kept {
                gauge!("zamsync_wal_oldest_event_timestamp_seconds").set(oldest_ms as f64 / 1000.0);
            }
            return Ok((0, 0));
        }

        // Merge tombstones back and sort by seq to restore WAL order.
        kept.extend(tombstones);
        kept.sort_unstable_by_key(|(seq, _)| *seq);

        // Rewrite WAL atomically via tmp + rename.
        let tmp = self.path.with_extension("wal.tmp");
        {
            let mut w = match &self.encryption {
                Some(key) => {
                    WalWriter::open_encrypted(&tmp, SequenceNumber::ZERO, Arc::clone(key))?
                }
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

        let size_after = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let bytes_freed = size_before.saturating_sub(size_after);

        // Reopen the writer on the new WAL.
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

        counter!("zamsync_events_expired_total").increment(dropped as u64);
        gauge!("zamsync_wal_size_bytes").set(size_after as f64);
        if let Some(oldest_ms) = oldest_phys_ms_kept {
            gauge!("zamsync_wal_oldest_event_timestamp_seconds").set(oldest_ms as f64 / 1000.0);
        }

        Ok((dropped, bytes_freed))
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
        let iter = scanner
            .scan()
            .filter_map(|res| -> Option<ZamResult<Event>> {
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

    fn byte_size(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zamsync_core::{Event, Hlc, NodeId, SequenceNumber};

    fn make_event(node: u32, seq: u64, phys_ms: u64, payload: Vec<u8>) -> Event {
        Event {
            origin_node: NodeId(node),
            seq: SequenceNumber(seq),
            hlc: Hlc {
                physical: phys_ms,
                logical: 0,
            },
            event_type: 1,
            payload,
        }
    }

    fn append_event(store: &mut WalEventStore, node: u32, seq: u64, phys_ms: u64) {
        let event = make_event(node, seq, phys_ms, format!("payload-{seq}").into_bytes());
        store.append(&event).unwrap();
    }

    // 1 day = 86_400_000 ms
    const DAY: u64 = 86_400_000;

    #[test]
    fn expire_empty_wal_returns_zero() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        // Do NOT create the WAL -- file doesn't exist
        let mut store = WalEventStore::open(&wal).unwrap();
        let (dropped, freed) = store.expire_before(1_000 * DAY, 0).unwrap();
        assert_eq!(dropped, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn expire_nothing_when_all_newer() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        let mut store = WalEventStore::open(&wal).unwrap();

        let now = 1_000 * DAY;
        append_event(&mut store, 1, 1, now);
        append_event(&mut store, 1, 2, now + DAY);
        store.sync().unwrap();

        // Cutoff is way in the past -- nothing qualifies
        let (dropped, freed) = store.expire_before(10 * DAY, 0).unwrap();
        assert_eq!(dropped, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn expire_drops_old_events() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        let mut store = WalEventStore::open(&wal).unwrap();

        let now = 1_000 * DAY;
        // 2 old events + 1 new event for node 1
        append_event(&mut store, 1, 1, now - 10 * DAY);
        append_event(&mut store, 1, 2, now - 5 * DAY);
        append_event(&mut store, 1, 3, now);
        store.sync().unwrap();

        let (dropped, freed) = store.expire_before(now - DAY, 0).unwrap();
        assert_eq!(dropped, 2);
        assert!(freed > 0);

        // Verify remaining events
        let remaining: Vec<_> = store.scan().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].seq.0, 3);
    }

    #[test]
    fn expire_min_keep_preserves_recent() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        let mut store = WalEventStore::open(&wal).unwrap();

        let now = 1_000 * DAY;
        // All 3 events are old
        append_event(&mut store, 1, 1, 10 * DAY);
        append_event(&mut store, 1, 2, 11 * DAY);
        append_event(&mut store, 1, 3, 12 * DAY);
        store.sync().unwrap();

        // Cutoff is past all events, but min_keep=2 should retain the 2 most recent
        let (dropped, _) = store.expire_before(now, 2).unwrap();
        assert_eq!(dropped, 1); // only seq=1 dropped

        let remaining: Vec<_> = store.scan().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(remaining.len(), 2);
        // seq 2 and 3 are kept (the last 2)
        let seqs: Vec<u64> = remaining.iter().map(|e| e.seq.0).collect();
        assert!(seqs.contains(&2));
        assert!(seqs.contains(&3));
    }

    #[test]
    fn expire_tombstones_always_preserved() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        let mut store = WalEventStore::open(&wal).unwrap();

        let now = 1_000 * DAY;
        // Write a tombstone (empty payload) with a very old seq
        store.writer.append_at_seq(SequenceNumber(1), &[]).unwrap();
        // Write one old real event
        append_event(&mut store, 1, 2, 10 * DAY);
        store.sync().unwrap();

        // Expire everything older than now -- tombstone must survive
        let (dropped, _) = store.expire_before(now, 0).unwrap();
        assert_eq!(dropped, 1); // only the real old event

        // Tombstone should still be in WAL via raw scan
        let scanner = WalScanner::open(&store.path).unwrap();
        let recs: Vec<_> = scanner.scan().collect();
        assert!(
            recs.iter().any(|r| r.as_ref().unwrap().payload.is_empty()),
            "tombstone must survive expiry"
        );
    }

    #[test]
    fn expire_multi_node_independent() {
        let dir = tempdir().unwrap();
        let wal = dir.path().join("events.wal");
        let mut store = WalEventStore::open(&wal).unwrap();

        let now = 1_000 * DAY;
        // Node 1: 1 old + 1 new
        append_event(&mut store, 1, 1, 10 * DAY);
        append_event(&mut store, 1, 2, now);
        // Node 2: 2 old
        append_event(&mut store, 2, 1, 10 * DAY);
        append_event(&mut store, 2, 2, 11 * DAY);
        store.sync().unwrap();

        let (dropped, _) = store.expire_before(now - DAY, 0).unwrap();
        assert_eq!(dropped, 3); // node1 old + node2 both

        let remaining: Vec<_> = store.scan().unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].origin_node.0, 1);
        assert_eq!(remaining[0].seq.0, 2);
    }
}
