use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zamsync_core::{Event, SequenceNumber, NodeId, Hlc, ZamResult, ZamError};
use crate::wal::{WalWriter, WalScanner};
use crate::state::StateStore;

pub struct ZamEngine<S: StateStore> {
    node_id: NodeId,
    writer: WalWriter,
    state: S,
    hlc: Hlc,
}

impl<S: StateStore> ZamEngine<S> {
    /// Initialise the engine with a specific node identity.
    pub fn open(path: impl AsRef<Path>, node_id: NodeId, mut state: S) -> ZamResult<Self> {
        let (last_seq, _recover_pos) = WalScanner::recover(&path)?;
        let mut max_hlc = Hlc::default();
        
        if last_seq.is_some() {
            let scanner = WalScanner::open(&path)?;
            for result in scanner.scan() {
                let record = result?;
                let event: Event = rkyv::from_bytes(&record.payload)
                    .map_err(|e| ZamError::Serialization(format!("Failed to deserialize event: {}", e)))?;
                if event.hlc > max_hlc {
                    max_hlc = event.hlc;
                }
                state.apply_event(record.seq, &event)?;
            }
        }

        let next_seq = last_seq.map(|s| s.next()).unwrap_or(SequenceNumber::ZERO);
        let writer = WalWriter::open(path, next_seq)?;

        Ok(Self { 
            node_id, 
            writer, 
            state,
            hlc: max_hlc,
        })
    }

    /// Submit a locally created event.
    pub fn submit(&mut self, event_type: u32, payload: Vec<u8>) -> ZamResult<SequenceNumber> {
        let next_seq = self.writer.next_seq();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        self.hlc.tick(now_ms);
        let event = Event {
            origin_node: self.node_id,
            seq: next_seq,
            hlc: self.hlc,
            event_type,
            payload,
        };

        self.apply_to_log(event)
    }

    /// Apply an event received from a peer.
    pub fn apply_replicated(&mut self, event: Event) -> ZamResult<SequenceNumber> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        self.hlc.sync(now_ms, &event.hlc);
        self.apply_to_log(event)
    }

    fn apply_to_log(&mut self, event: Event) -> ZamResult<SequenceNumber> {
        let bytes = rkyv::to_bytes::<_, 1024>(&event)
            .map_err(|e| ZamError::Serialization(format!("Failed to serialize event: {}", e)))?;
        
        let seq = self.writer.append(&bytes)?;
        self.state.apply_event(seq, &event)?;
        
        Ok(seq)
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn sync(&mut self) -> ZamResult<()> {
        self.writer.sync().map_err(Into::into)
    }
}
