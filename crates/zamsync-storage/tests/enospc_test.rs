use zamsync_core::ports::{EventStore, StateStore};
use zamsync_core::{Event, NodeId, SequenceNumber, ZamError, ZamResult};
use zamsync_storage::ZamEngine;
use zamsync_testing::{InMemoryEventStore, InMemoryPeerStore};

// Event store that succeeds for the first `max` appends, then returns ENOSPC.
struct FailAfterN {
    inner: InMemoryEventStore,
    max: usize,
}

impl FailAfterN {
    fn new(max: usize) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            max,
        }
    }
}

impl EventStore for FailAfterN {
    fn next_seq(&self) -> SequenceNumber {
        self.inner.next_seq()
    }

    fn append(&mut self, event: &Event) -> ZamResult<SequenceNumber> {
        if self.inner.events().len() >= self.max {
            return Err(ZamError::Io(std::io::Error::other(
                "no space left on device",
            )));
        }
        self.inner.append(event)
    }

    fn scan(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>> {
        self.inner.scan()
    }

    fn sync(&mut self) -> ZamResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Counter {
    count: usize,
}

impl StateStore for Counter {
    fn apply_event(&mut self, _seq: SequenceNumber, _event: &Event) -> ZamResult<()> {
        self.count += 1;
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

/// When the underlying store returns an I/O error (ENOSPC), `submit()` must
/// propagate the error and leave the engine state fully consistent:
/// no phantom events in the state projection, no corrupted sequence counter.
#[test]
fn test_submit_enospc_does_not_corrupt_state() {
    const BEFORE_FULL: usize = 5;
    let node_id = NodeId(1);

    let event_store = FailAfterN::new(BEFORE_FULL);
    let peer_store = InMemoryPeerStore::new(node_id);
    let mut engine = ZamEngine::new(node_id, event_store, peer_store, Counter::default()).unwrap();

    // Fill the store to capacity
    for i in 0..BEFORE_FULL {
        engine
            .submit(1, format!("event-{i}").into_bytes())
            .unwrap_or_else(|e| panic!("submit {i} failed: {e}"));
    }
    assert_eq!(engine.state().count, BEFORE_FULL);

    // Next submit must fail with an I/O error
    let err = engine
        .submit(1, b"overflow".to_vec())
        .expect_err("submit must fail when store is full");
    assert!(
        matches!(err, ZamError::Io(_)),
        "expected Io error, got: {err:?}"
    );

    // State must not be corrupted: exactly BEFORE_FULL events, no phantom
    assert_eq!(
        engine.state().count,
        BEFORE_FULL,
        "state must reflect only committed events after ENOSPC"
    );

    // Scan must return exactly BEFORE_FULL events with unique seqs
    let events: Vec<Event> = engine
        .scan_events()
        .unwrap()
        .filter_map(|r: ZamResult<Event>| r.ok())
        .collect();
    assert_eq!(
        events.len(),
        BEFORE_FULL,
        "store must contain exactly the committed events"
    );
    let mut seqs: Vec<u64> = events.iter().map(|e| e.seq.0).collect();
    seqs.sort_unstable();
    seqs.dedup();
    assert_eq!(
        seqs.len(),
        BEFORE_FULL,
        "no duplicate sequences after ENOSPC"
    );

    // Submit after the failure must still fail (store is still full)
    assert!(
        engine.submit(1, b"still-full".to_vec()).is_err(),
        "subsequent submit must also fail while store is full"
    );
}
