use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, SyncMessage, VersionVector, ZamResult};
use zamsync_storage::ZamEngine;
use zamsync_testing::{run_direct_sync, InMemoryEventStore, InMemoryPeerStore};

#[derive(Default)]
struct NoopState;

impl StateStore for NoopState {
    fn apply_event(&mut self, _seq: SequenceNumber, _event: &Event) -> ZamResult<()> {
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

fn make_engine(
    node_id: NodeId,
) -> ZamResult<ZamEngine<InMemoryEventStore, InMemoryPeerStore, NoopState>> {
    ZamEngine::new(
        node_id,
        InMemoryEventStore::default(),
        InMemoryPeerStore::new(node_id),
        NoopState,
    )
}

#[test]
fn test_direct_sync_convergence() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let mut engine_a = make_engine(node_a)?;
    let mut engine_b = make_engine(node_b)?;

    engine_a.submit(1, b"a-event-1".to_vec())?;
    engine_a.submit(1, b"a-event-2".to_vec())?;
    engine_b.submit(1, b"b-event-1".to_vec())?;

    let (applied_to_a, applied_to_b) = run_direct_sync(&mut engine_a, &mut engine_b)?;

    assert_eq!(applied_to_a, 1, "A should receive 1 event from B");
    assert_eq!(applied_to_b, 2, "B should receive 2 events from A");

    let vv_a = engine_a.replication_state().local_vv.clone();
    let vv_b = engine_b.replication_state().local_vv.clone();
    assert_eq!(
        vv_a.entries.get(&node_a.0),
        vv_b.entries.get(&node_a.0),
        "VV for node A must agree"
    );
    assert_eq!(
        vv_a.entries.get(&node_b.0),
        vv_b.entries.get(&node_b.0),
        "VV for node B must agree"
    );

    Ok(())
}

#[test]
fn test_direct_sync_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let mut engine_a = make_engine(node_a)?;
    let mut engine_b = make_engine(node_b)?;

    engine_a.submit(1, b"x".to_vec())?;

    run_direct_sync(&mut engine_a, &mut engine_b)?;
    let (a2, b2) = run_direct_sync(&mut engine_a, &mut engine_b)?;

    assert_eq!(a2, 0, "second sync should transfer nothing to A");
    assert_eq!(b2, 0, "second sync should transfer nothing to B");

    Ok(())
}

#[test]
fn test_handle_sync_message_handshake() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let mut engine_a = make_engine(node_a)?;
    let engine_b = make_engine(node_b)?;

    engine_a.submit(1, b"e1".to_vec())?;
    engine_a.submit(1, b"e2".to_vec())?;
    engine_a.submit(1, b"e3".to_vec())?;

    let peer_vv = engine_b.replication_state().local_vv.clone();
    let handshake = SyncMessage::Handshake {
        node_id: node_b,
        vv: peer_vv,
    };

    let responses = engine_a.handle_sync_message(node_b, handshake)?;

    assert!(
        responses.len() >= 2,
        "expected at least Handshake + SyncComplete, got {}",
        responses.len()
    );

    let has_handshake = responses
        .iter()
        .any(|m| matches!(m, SyncMessage::Handshake { .. }));
    assert!(has_handshake, "response must include a Handshake");

    let total_events: usize = responses
        .iter()
        .filter_map(|m| {
            if let SyncMessage::EventBatch { events, .. } = m {
                Some(events.len())
            } else {
                None
            }
        })
        .sum();
    assert_eq!(total_events, 3, "expected all 3 events in response batches");

    let last = responses.last().unwrap();
    assert!(
        matches!(last, SyncMessage::SyncComplete),
        "last message must be SyncComplete"
    );

    Ok(())
}

#[test]
fn test_submit_hlc_strictly_monotonic() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = make_engine(NodeId(1))?;

    for i in 0..30u32 {
        engine.submit(i, vec![i as u8])?;
    }

    let events: Vec<Event> = engine.scan_events()?.collect::<ZamResult<_>>()?;
    assert_eq!(events.len(), 30);

    for window in events.windows(2) {
        assert!(
            window[1].hlc > window[0].hlc,
            "HLC must be strictly monotonic: {:?} not > {:?}",
            window[1].hlc,
            window[0].hlc
        );
    }
    Ok(())
}

#[test]
fn test_apply_replicated_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let mut engine_a = make_engine(node_a)?;
    let mut engine_b = make_engine(node_b)?;

    engine_a.submit(1, b"patient-P001".to_vec())?;
    engine_a.submit(1, b"patient-P002".to_vec())?;

    let events: Vec<Event> = engine_a.scan_events()?.collect::<ZamResult<_>>()?;
    assert_eq!(events.len(), 2);

    // Apply the same two events to B three times -- only one copy must be stored.
    for _ in 0..3 {
        for e in &events {
            engine_b.apply_replicated(e.clone())?;
        }
    }

    let b_events: Vec<Event> = engine_b.scan_events()?.collect::<ZamResult<_>>()?;
    assert_eq!(b_events.len(), 2, "repeated apply_replicated must be idempotent");
    assert_eq!(b_events[0].payload, b"patient-P001");
    assert_eq!(b_events[1].payload, b"patient-P002");

    // VV on B must reflect A's highest seq exactly once, not inflated.
    let vv = &engine_b.replication_state().local_vv;
    assert_eq!(vv.get(node_a), events.last().unwrap().seq);

    Ok(())
}

#[test]
fn test_handle_sync_message_empty_handshake() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let mut engine_a = make_engine(node_a)?;

    engine_a.submit(1, b"x".to_vec())?;

    let mut full_vv = VersionVector::default();
    full_vv.update(node_a, SequenceNumber(0));

    let responses = engine_a.handle_sync_message(
        node_b,
        SyncMessage::Handshake {
            node_id: node_b,
            vv: full_vv,
        },
    )?;

    let total_events: usize = responses
        .iter()
        .filter_map(|m| {
            if let SyncMessage::EventBatch { events, .. } = m {
                Some(events.len())
            } else {
                None
            }
        })
        .sum();
    assert_eq!(
        total_events, 0,
        "peer is up-to-date, no events should be sent"
    );
    assert!(matches!(responses.last(), Some(SyncMessage::SyncComplete)));

    Ok(())
}
