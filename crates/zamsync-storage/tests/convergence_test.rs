use std::collections::HashMap;
use tempfile::tempdir;
use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_storage::{FilePeerStore, LogSorter, WalEventStore, ZamEngine};
use zamsync_testing::run_direct_sync;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct KVState {
    data: HashMap<String, String>,
    history: Vec<String>,
}

impl StateStore for KVState {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        let val = String::from_utf8_lossy(&event.payload).to_string();
        self.data
            .insert(format!("node_{}", event.origin_node.0), val.clone());
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

    assert_eq!(
        final_state_a.history, final_state_b.history,
        "convergence path diverged"
    );
    assert_eq!(
        final_state_a, final_state_b,
        "final states are not identical"
    );

    Ok(())
}

#[test]
fn test_three_node_split_brain_convergence() -> Result<(), Box<dyn std::error::Error>> {
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    let dir_c = tempdir()?;

    let mut engine_a = ZamEngine::open_wal(dir_a.path(), NodeId(1), KVState::default())?;
    let mut engine_b = ZamEngine::open_wal(dir_b.path(), NodeId(2), KVState::default())?;
    let mut engine_c = ZamEngine::open_wal(dir_c.path(), NodeId(3), KVState::default())?;

    // Full partition: each node produces events with no knowledge of the others.
    engine_a.submit(1, b"A1".to_vec())?;
    engine_a.submit(1, b"A2".to_vec())?;
    engine_b.submit(1, b"B1".to_vec())?;
    engine_c.submit(1, b"C1".to_vec())?;
    engine_c.submit(1, b"C2".to_vec())?;
    engine_c.submit(1, b"C3".to_vec())?;

    // Full mesh sync in one pass: A↔B → B↔C → A↔C.
    // After A↔B: both have {A1,A2,B1}.
    // After B↔C: both have {A1,A2,B1,C1,C2,C3}.
    // After A↔C: A learns {C1,C2,C3} from C.
    run_direct_sync(&mut engine_a, &mut engine_b)?;
    run_direct_sync(&mut engine_b, &mut engine_c)?;
    run_direct_sync(&mut engine_a, &mut engine_c)?;

    // Every node must hold all 6 events.
    let count_a = engine_a.scan_events()?.count();
    let count_b = engine_b.scan_events()?.count();
    let count_c = engine_c.scan_events()?.count();
    assert_eq!(count_a, 6, "A must have all 6 events after full mesh sync");
    assert_eq!(count_b, 6, "B must have all 6 events after full mesh sync");
    assert_eq!(count_c, 6, "C must have all 6 events after full mesh sync");

    // Determinism: sorted event streams must be identical on all three nodes.
    let sorted_a: Vec<Vec<u8>> = engine_a.sorted_scan()?.map(|r| r.unwrap().payload).collect();
    let sorted_b: Vec<Vec<u8>> = engine_b.sorted_scan()?.map(|r| r.unwrap().payload).collect();
    let sorted_c: Vec<Vec<u8>> = engine_c.sorted_scan()?.map(|r| r.unwrap().payload).collect();

    assert_eq!(sorted_a, sorted_b, "A and B must converge to identical sorted streams");
    assert_eq!(sorted_b, sorted_c, "B and C must converge to identical sorted streams");

    // VVs must agree on every node's highest seq.
    let vv_a = engine_a.replication_state().local_vv.clone();
    let vv_b = engine_b.replication_state().local_vv.clone();
    let vv_c = engine_c.replication_state().local_vv.clone();
    for node_id in [NodeId(1), NodeId(2), NodeId(3)] {
        let seq_a = vv_a.get(node_id);
        let seq_b = vv_b.get(node_id);
        let seq_c = vv_c.get(node_id);
        assert_eq!(seq_a, seq_b, "VV for node {} must agree: A={:?} B={:?}", node_id.0, seq_a, seq_b);
        assert_eq!(seq_b, seq_c, "VV for node {} must agree: B={:?} C={:?}", node_id.0, seq_b, seq_c);
    }

    Ok(())
}
