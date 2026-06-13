use std::sync::mpsc;
use tempfile::tempdir;
use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_network::TcpTransport;
use zamsync_storage::{FilePeerStore, SyncSession, WalEventStore, ZamEngine};

#[derive(Default)]
struct EventCounter {
    count: usize,
}

impl StateStore for EventCounter {
    fn apply_event(&mut self, _seq: SequenceNumber, _event: &Event) -> ZamResult<()> {
        self.count += 1;
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

fn open_engine(
    dir: &tempfile::TempDir,
    node_id: NodeId,
) -> ZamResult<ZamEngine<WalEventStore, FilePeerStore, EventCounter>> {
    ZamEngine::open_wal(dir.path(), node_id, EventCounter::default())
}

/// Two nodes sync over a loopback TCP connection.
/// A serves (responder), B initiates (initiator).
/// After sync both must have the same number of events.
#[test]
fn test_tcp_sync_end_to_end() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(1);
    let node_b = NodeId(2);

    let dir_a = tempdir()?;
    let dir_b = tempdir()?;

    // Pre-populate both engines before connecting.
    {
        let mut engine = open_engine(&dir_a, node_a)?;
        engine.submit(1, b"a-event-1".to_vec())?;
        engine.submit(1, b"a-event-2".to_vec())?;
        engine.sync()?;
    }
    {
        let mut engine = open_engine(&dir_b, node_b)?;
        engine.submit(1, b"b-event-1".to_vec())?;
        engine.sync()?;
    }

    // A binds first; send the port over a channel so B knows where to connect.
    let (port_tx, port_rx) = mpsc::channel::<u16>();
    let dir_a_path = dir_a.path().to_path_buf();

    let handle_a = std::thread::spawn(move || -> ZamResult<usize> {
        let mut engine = ZamEngine::open_wal(dir_a_path, node_a, EventCounter::default())?;
        let mut transport = TcpTransport::bind("127.0.0.1:0")?;
        port_tx
            .send(transport.local_addr()?.port())
            .expect("port channel closed");
        transport.accept_peer(node_b)?;
        SyncSession::new(&mut engine, &mut transport).serve_one(node_b)?;
        Ok(engine.state().count)
    });

    let port = port_rx.recv()?;

    let mut engine_b = ZamEngine::open_wal(dir_b.path(), node_b, EventCounter::default())?;
    let mut transport_b = TcpTransport::bind("127.0.0.1:0")?;
    transport_b.connect(node_a, &format!("127.0.0.1:{}", port))?;

    let stats_b = SyncSession::new(&mut engine_b, &mut transport_b).sync(node_a)?;

    let count_a = handle_a.join().expect("thread A panicked")?;
    let count_b = engine_b.state().count;

    assert_eq!(count_a, 3, "A should have 3 events after sync");
    assert_eq!(count_b, 3, "B should have 3 events after sync");
    assert_eq!(
        stats_b.events_received, 2,
        "B should receive 2 events from A"
    );
    assert_eq!(stats_b.events_sent, 1, "B should send 1 event to A");

    Ok(())
}

/// Syncing again after convergence transfers zero events.
#[test]
fn test_tcp_sync_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let node_a = NodeId(10);
    let node_b = NodeId(11);

    let dir_a = tempdir()?;
    let dir_b = tempdir()?;

    // First sync
    run_one_sync(&dir_a, node_a, &dir_b, node_b)?;

    // Second sync -- nothing to transfer
    let (sent, received) = run_one_sync(&dir_a, node_a, &dir_b, node_b)?;
    assert_eq!(sent, 0, "no events should be sent on second sync");
    assert_eq!(received, 0, "no events should be received on second sync");

    Ok(())
}

/// Syncing more than EVENTS_PER_BATCH events exercises the chunked-send path.
/// All events must arrive intact even when split across multiple frames.
#[test]
fn test_tcp_sync_large_batch() -> Result<(), Box<dyn std::error::Error>> {
    use zamsync_storage::EVENTS_PER_BATCH;

    let node_a = NodeId(20);
    let node_b = NodeId(21);
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;

    // A writes more than one batch worth of events
    let event_count = EVENTS_PER_BATCH + 50;
    {
        let mut engine = ZamEngine::open_wal(dir_a.path(), node_a, EventCounter::default())?;
        for i in 0..event_count {
            engine.submit(1, format!("event-{i}").into_bytes())?;
        }
        engine.sync()?;
    }

    let (sent, received) = run_one_sync(&dir_a, node_a, &dir_b, node_b)?;

    assert_eq!(received, event_count, "B must receive all events from A");
    assert_eq!(sent, 0, "B has nothing to send");

    // B must be queryable and have the right count
    let engine_b = ZamEngine::open_wal(dir_b.path(), node_b, EventCounter::default())?;
    assert_eq!(engine_b.state().count, event_count);

    Ok(())
}

fn run_one_sync(
    dir_a: &tempfile::TempDir,
    node_a: NodeId,
    dir_b: &tempfile::TempDir,
    node_b: NodeId,
) -> ZamResult<(usize, usize)> {
    let (port_tx, port_rx) = mpsc::channel::<u16>();
    let dir_a_path = dir_a.path().to_path_buf();

    let handle_a = std::thread::spawn(move || -> ZamResult<()> {
        let mut engine = ZamEngine::open_wal(dir_a_path, node_a, EventCounter::default())?;
        let mut transport = TcpTransport::bind("127.0.0.1:0")?;
        port_tx
            .send(transport.local_addr()?.port())
            .expect("port channel closed");
        transport.accept_peer(node_b)?;
        SyncSession::new(&mut engine, &mut transport).serve_one(node_b)?;
        Ok(())
    });

    let port = port_rx
        .recv()
        .map_err(|_| zamsync_core::ZamError::Protocol("port channel closed".into()))?;

    let mut engine_b = ZamEngine::open_wal(dir_b.path(), node_b, EventCounter::default())?;
    let mut transport_b = TcpTransport::bind("127.0.0.1:0")?;
    transport_b.connect(node_a, &format!("127.0.0.1:{}", port))?;

    let stats = SyncSession::new(&mut engine_b, &mut transport_b).sync(node_a)?;

    handle_a.join().expect("thread A panicked")?;

    Ok((stats.events_sent, stats.events_received))
}
