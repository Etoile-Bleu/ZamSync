use tempfile::tempdir;
use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_storage::{FilePeerStore, WalEventStore, ZamEngine};
use zamsync_testing::run_direct_sync;

#[derive(Default)]
struct Counter(usize);

impl StateStore for Counter {
    fn apply_event(&mut self, _seq: SequenceNumber, _event: &Event) -> ZamResult<()> {
        self.0 += 1;
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

fn open_engine(
    dir: &tempfile::TempDir,
    node: NodeId,
) -> ZamResult<ZamEngine<WalEventStore, FilePeerStore, Counter>> {
    ZamEngine::open_wal(dir.path(), node, Counter::default())
}

/// After syncing with all peers and compact(), WAL records below the frontier
/// are removed and new submits continue with the correct sequence number.
#[test]
fn test_compact_drops_confirmed_events() -> ZamResult<()> {
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    let node_a = NodeId(1);
    let node_b = NodeId(2);
    let wal_path = dir_a.path().join("events.wal");

    // A submits 5 events
    let next_seq = {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        for i in 0..5u32 {
            engine_a.submit(1, format!("event-{i}").into_bytes())?;
        }
        engine_a.sync()?;
        let next = engine_a.replication_state().local_vv.get(node_a).next();
        next
    };

    // B syncs from A -- this makes A aware that B has confirmed all 5 events
    {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        let mut engine_b = open_engine(&dir_b, node_b)?;
        run_direct_sync(&mut engine_a, &mut engine_b)?;
        engine_a.sync()?;
        engine_b.sync()?;
    }

    // Capture WAL size before compaction
    let wal_size_before = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);

    // Compact A's WAL -- all 5 events from A have been confirmed by B.
    // B's replicated events (received during sync) will not be compacted
    // because A has not confirmed them back to B.
    let dropped = {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        let d = engine_a.compact()?;
        engine_a.sync()?;
        d
    };
    assert_eq!(dropped, 5, "all 5 events from A should be dropped");

    // WAL file must be smaller (B's replicated events remain, but A's 5 are gone)
    let wal_size_after = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
    assert!(
        wal_size_after < wal_size_before,
        "WAL must shrink after compaction (was {wal_size_before}, now {wal_size_after})"
    );

    // Submit a new event -- seq must continue from before compaction
    let mut engine_a = open_engine(&dir_a, node_a)?;
    let new_seq = engine_a.submit(1, b"after-compact".to_vec())?;
    assert_eq!(
        new_seq, next_seq,
        "next submit seq must continue from pre-compaction frontier"
    );
    engine_a.sync()?;

    // Reopen and verify events are visible (replicated B events + 1 new A event)
    let final_engine = open_engine(&dir_a, node_a)?;
    assert!(
        final_engine.state().0 >= 1,
        "at least the post-compact event must be visible"
    );

    Ok(())
}

/// After compaction, syncing with a new peer still works correctly.
/// The compacted node can still serve events that the new peer doesn't have.
#[test]
fn test_compact_then_sync_new_peer() -> ZamResult<()> {
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    let dir_c = tempdir()?;
    let node_a = NodeId(10);
    let node_b = NodeId(11);
    let node_c = NodeId(12);

    // A submits, B syncs, A compacts
    {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        engine_a.submit(1, b"old-event".to_vec())?;
        engine_a.sync()?;
    }
    {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        let mut engine_b = open_engine(&dir_b, node_b)?;
        run_direct_sync(&mut engine_a, &mut engine_b)?;
        engine_a.sync()?;
    }
    {
        let mut engine_a = open_engine(&dir_a, node_a)?;
        engine_a.compact()?;
        engine_a.submit(1, b"new-event".to_vec())?;
        engine_a.sync()?;
    }

    // C syncs with A -- C is brand new, A has been compacted
    // C should receive at least the post-compact event
    let mut engine_a = open_engine(&dir_a, node_a)?;
    let mut engine_c = open_engine(&dir_c, node_c)?;
    run_direct_sync(&mut engine_a, &mut engine_c)?;

    // C must have the post-compact event
    assert!(
        engine_c.state().0 >= 1,
        "C must receive at least the post-compact event"
    );

    Ok(())
}
