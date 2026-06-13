use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_core::ports::StateStore;
use zamsync_storage::{FilePeerStore, LogSorter, WalEventStore, ZamEngine};
use tempfile::tempdir;
use std::collections::HashMap;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct KVState {
    data: HashMap<String, String>,
    history: Vec<String>,
}

impl StateStore for KVState {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        let val = String::from_utf8_lossy(&event.payload).to_string();
        self.data.insert(format!("node_{}", event.origin_node.0), val.clone());
        self.history.push(val);
        Ok(())
    }

    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

#[test]
fn test_split_brain_convergence() -> Result<(), Box<dyn std::error::Error>> {
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    let node_a_id = NodeId(1);
    let node_b_id = NodeId(2);

    let make_engine = |dir: &tempfile::TempDir, node_id: NodeId| -> ZamResult<_> {
        let event_store = WalEventStore::open(dir.path().join("events.wal"))?;
        let peer_store = FilePeerStore::open(dir.path().join("peers.state"), node_id)?;
        ZamEngine::new(node_id, event_store, peer_store, KVState::default())
    };

    let mut engine_a = make_engine(&dir_a, node_a_id)?;
    let mut engine_b = make_engine(&dir_b, node_b_id)?;

    engine_a.submit(1, b"A1".to_vec())?;
    engine_a.submit(1, b"A2".to_vec())?;
    engine_b.submit(1, b"B1".to_vec())?;
    engine_b.submit(1, b"B2".to_vec())?;

    let events_a: Vec<Event> = engine_a.scan_events()?.collect::<ZamResult<_>>()?;
    let events_b: Vec<Event> = engine_b.scan_events()?.collect::<ZamResult<_>>()?;

    let mut final_state_a = KVState::default();
    let sorter_a = LogSorter::new(vec![
        events_a.clone().into_iter().map(Ok),
        events_b.clone().into_iter().map(Ok),
    ])?;
    for (i, event_res) in sorter_a.enumerate() {
        final_state_a.apply_event(SequenceNumber(i as u64), &event_res?)?;
    }

    let mut final_state_b = KVState::default();
    let sorter_b = LogSorter::new(vec![
        events_b.clone().into_iter().map(Ok),
        events_a.clone().into_iter().map(Ok),
    ])?;
    for (i, event_res) in sorter_b.enumerate() {
        final_state_b.apply_event(SequenceNumber(i as u64), &event_res?)?;
    }

    assert_eq!(final_state_a.history, final_state_b.history, "convergence path diverged");
    assert_eq!(final_state_a, final_state_b, "final states are not identical");

    Ok(())
}
