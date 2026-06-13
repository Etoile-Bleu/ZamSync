use zamsync_core::{Event, Hlc, NodeId, SequenceNumber, ZamResult};
use zamsync_core::ports::StateStore;
use zamsync_storage::{FilePeerStore, WalEventStore, ZamEngine};
use tempfile::tempdir;
use std::collections::HashMap;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
enum DomainEvent {
    UpsertRecord { id: String, value: String },
}

#[derive(Default)]
struct RecordStore {
    records: HashMap<String, String>,
    last_seq: Option<SequenceNumber>,
}

impl StateStore for RecordStore {
    fn apply_event(&mut self, seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        if event.event_type == 1 {
            let domain: DomainEvent = serde_json::from_slice(&event.payload)
                .map_err(|e| zamsync_core::ZamError::Serialization(e.to_string()))?;
            match domain {
                DomainEvent::UpsertRecord { id, value } => {
                    self.records.insert(id, value);
                }
            }
        }
        self.last_seq = Some(seq);
        Ok(())
    }

    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        self.last_seq
    }
}

fn open_engine(
    dir: &tempfile::TempDir,
    node_id: NodeId,
) -> ZamResult<ZamEngine<WalEventStore, FilePeerStore, RecordStore>> {
    let event_store = WalEventStore::open(dir.path().join("events.wal"))?;
    let peer_store = FilePeerStore::open(dir.path().join("peers.state"), node_id)?;
    ZamEngine::new(node_id, event_store, peer_store, RecordStore::default())
}

#[test]
fn test_engine_distributed_identity() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let self_id = NodeId(1);

    let mut engine = open_engine(&dir, self_id)?;

    let payload = serde_json::to_vec(&DomainEvent::UpsertRecord {
        id: "r1".into(),
        value: "hello".into(),
    })?;
    let seq = engine.submit(1, payload)?;
    assert_eq!(seq.0, 0);
    assert_eq!(engine.state().records["r1"], "hello");

    drop(engine);

    let recovered = open_engine(&dir, self_id)?;
    assert_eq!(recovered.state().records["r1"], "hello");

    Ok(())
}

#[test]
fn test_replicated_event_handling() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let self_id = NodeId(1);
    let peer_id = NodeId(2);

    let mut engine = open_engine(&dir, self_id)?;

    let event = Event {
        origin_node: peer_id,
        seq: SequenceNumber(100),
        hlc: Hlc::new(12345, 0),
        event_type: 1,
        payload: serde_json::to_vec(&DomainEvent::UpsertRecord {
            id: "r2".into(),
            value: "world".into(),
        })?,
    };

    engine.apply_replicated(event)?;
    assert_eq!(engine.state().records["r2"], "world");

    Ok(())
}
