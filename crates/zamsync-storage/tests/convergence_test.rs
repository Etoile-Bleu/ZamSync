use zamsync_core::{Event, SequenceNumber, NodeId, ZamResult, Hlc};
use zamsync_storage::{ZamEngine, StateStore, LogSorter, WalScanner};
use tempfile::tempdir;
use std::collections::HashMap;

// --- A Simple Counter/Map State Store for Testing ---
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
        None // Not used in this specific test
    }
}

#[test]
fn test_split_brain_convergence() -> Result<(), Box<dyn std::error::Error>> {
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    let path_a = dir_a.path().join("a.wal");
    let path_b = dir_b.path().join("b.wal");

    let node_a_id = NodeId(1);
    let node_b_id = NodeId(2);

    // 1. Initial State: Nodes are isolated
    let mut engine_a = ZamEngine::open(&path_a, node_a_id, KVState::default())?;
    let mut engine_b = ZamEngine::open(&path_b, node_b_id, KVState::default())?;

    // 2. Concurrent Updates
    // Node A creates 2 events
    engine_a.submit(1, b"A1".to_vec())?;
    engine_a.submit(1, b"A2".to_vec())?;

    // Node B creates 2 events
    engine_b.submit(1, b"B1".to_vec())?;
    engine_b.submit(1, b"B2".to_vec())?;

    // 3. Simulate "Sync" by reading events from each other's WAL
    let scan_a = WalScanner::open(&path_a)?;
    let events_a: Vec<Event> = scan_a.scan()
        .map(|res| {
            let record = res?;
            rkyv::from_bytes::<Event>(&record.payload).map_err(|e| zamsync_core::ZamError::Serialization(e.to_string()))
        })
        .collect::<ZamResult<Vec<_>>>()?;

    let scan_b = WalScanner::open(&path_b)?;
    let events_b: Vec<Event> = scan_b.scan()
        .map(|res| {
            let record = res?;
            rkyv::from_bytes::<Event>(&record.payload).map_err(|e| zamsync_core::ZamError::Serialization(e.to_string()))
        })
        .collect::<ZamResult<Vec<_>>>()?;

    // 4. Convergence Replay
    // Both nodes merge ALL events (A1, A2, B1, B2) using LogSorter
    // This simulates what happens when a node wants to reconstruct the "Global Truth"
    
    let mut final_state_a = KVState::default();
    let sorter_a = LogSorter::new(vec![
        events_a.clone().into_iter().map(Ok), 
        events_b.clone().into_iter().map(Ok)
    ])?;
    for (i, event_res) in sorter_a.enumerate() {
        final_state_a.apply_event(SequenceNumber(i as u64), &event_res?)?;
    }

    let mut final_state_b = KVState::default();
    let sorter_b = LogSorter::new(vec![
        events_b.clone().into_iter().map(Ok), 
        events_a.clone().into_iter().map(Ok)
    ])?;
    for (i, event_res) in sorter_b.enumerate() {
        final_state_b.apply_event(SequenceNumber(i as u64), &event_res?)?;
    }

    // 5. THE ULTIMATE ASSERTION
    // The history of applications MUST be identical
    assert_eq!(final_state_a.history, final_state_b.history, "Path to convergence diverged!");
    assert_eq!(final_state_a, final_state_b, "Final states are not identical!");

    println!("Convergence Path: {:?}", final_state_a.history);

    Ok(())
}
