use crate::protocol;
use std::io::BufWriter;
use std::net::TcpStream;
use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, ZamError, ZamResult};

/// A single-connection transport returned by [`super::TcpTransport::accept_split`].
///
/// Owns exactly one peer stream and implements [`Transport`] for that peer.
/// `Send`-safe: move it into a worker thread so the hub can serve N peers
/// concurrently without blocking the accept loop.
pub struct TcpPeerTransport {
    peer_id: NodeId,
    stream: TcpStream,
    frame_buf: protocol::FrameBuffer,
    pending: Option<SyncMessage>,
}

impl TcpPeerTransport {
    pub(super) fn new(peer_id: NodeId, stream: TcpStream, pending: Option<SyncMessage>) -> Self {
        Self {
            peer_id,
            stream,
            frame_buf: protocol::FrameBuffer::new(),
            pending,
        }
    }

    /// NodeId extracted from the peer's opening Handshake.
    pub fn peer_id(&self) -> NodeId {
        self.peer_id
    }
}

impl Transport for TcpPeerTransport {
    fn send(&mut self, _peer_id: NodeId, message: &SyncMessage) -> ZamResult<usize> {
        let mut writer = BufWriter::new(&self.stream);
        protocol::encode(message, &mut writer)
    }

    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>> {
        if let Some(msg) = self.pending.take() {
            return Ok(Some((self.peer_id, msg)));
        }
        match self.frame_buf.try_read_frame(&mut self.stream) {
            Ok(Some(bytes)) => {
                let msg = rkyv::from_bytes::<SyncMessage>(&bytes)
                    .map_err(|e| ZamError::Serialization(format!("{}", e)))?;
                Ok(Some((self.peer_id, msg)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::TcpTransport;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use zamsync_core::ports::StateStore;
    use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
    use zamsync_storage::{SyncSession, ZamEngine};

    #[derive(Default)]
    struct Counter {
        pub count: usize,
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

    // ---- helpers ---------------------------------------------------------------

    fn spawn_hub_once(
        hub_dir: std::path::PathBuf,
        hub_id: NodeId,
    ) -> (String, std::thread::JoinHandle<usize>) {
        let mut hub_transport = TcpTransport::bind("127.0.0.1:0").unwrap();
        let addr = hub_transport.local_addr().unwrap().to_string();
        let h = thread::spawn(move || {
            let mut pt = hub_transport.accept_split().unwrap();
            let peer_id = pt.peer_id();
            let mut eng = ZamEngine::open_wal(&hub_dir, hub_id, Counter::default()).unwrap();
            SyncSession::new(&mut eng, &mut pt)
                .serve_one(peer_id)
                .unwrap();
            eng.sync().unwrap();
            eng.state().count
        });
        (addr, h)
    }

    // ---- bandwidth budget tests ------------------------------------------------

    /// bytes_sent is non-zero for a sync with no data events (control frames).
    #[test]
    fn test_bytes_sent_nonzero_for_empty_sync() {
        let hub_dir = tempfile::tempdir().unwrap();
        let clinic_dir = tempfile::tempdir().unwrap();
        let hub_id = NodeId(10);
        let clinic_id = NodeId(11);

        ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();
        ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();

        let (addr, hub_h) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);

        let mut transport = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        transport.connect(hub_id, &addr).unwrap();
        let stats = SyncSession::new(&mut eng, &mut transport)
            .sync(hub_id)
            .unwrap();
        hub_h.join().unwrap();

        assert!(
            stats.bytes_sent > 0,
            "handshake + SyncComplete alone must produce non-zero bytes_sent"
        );
        assert!(!stats.budget_exhausted);
    }

    /// bytes_sent is strictly larger when the initiator has events to send.
    ///
    /// The budget cap applies to bytes the initiator *sends*, so we populate
    /// the clinic (initiator) with events and leave the hub empty.
    #[test]
    fn test_bytes_sent_grows_with_data() {
        let hub_id = NodeId(20);
        let clinic_id = NodeId(21);

        // ---- Empty initiator baseline -------------------------------------------
        let empty_hub_dir = tempfile::tempdir().unwrap();
        let empty_clinic_dir = tempfile::tempdir().unwrap();
        ZamEngine::open_wal(empty_hub_dir.path(), hub_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();
        ZamEngine::open_wal(empty_clinic_dir.path(), clinic_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();

        let (addr_empty, h_empty) = spawn_hub_once(empty_hub_dir.path().to_path_buf(), hub_id);
        let mut t_empty = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng_empty =
            ZamEngine::open_wal(empty_clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        t_empty.connect(hub_id, &addr_empty).unwrap();
        let stats_empty = SyncSession::new(&mut eng_empty, &mut t_empty)
            .sync(hub_id)
            .unwrap();
        h_empty.join().unwrap();

        // ---- Initiator with 50 events -------------------------------------------
        let hub_dir = tempfile::tempdir().unwrap();
        let clinic_dir = tempfile::tempdir().unwrap();
        ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();
        {
            let mut eng =
                ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
            for i in 0..50usize {
                eng.submit(1, format!("bytes-grow-evt-{i}").into_bytes())
                    .unwrap();
            }
            eng.sync().unwrap();
        }

        let (addr_data, h_data) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);
        let mut t_data = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng_data =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        t_data.connect(hub_id, &addr_data).unwrap();
        let stats_data = SyncSession::new(&mut eng_data, &mut t_data)
            .sync(hub_id)
            .unwrap();
        h_data.join().unwrap();

        assert!(
            stats_data.bytes_sent > stats_empty.bytes_sent,
            "clinic with 50 events ({} B) must send more bytes than empty clinic ({} B)",
            stats_data.bytes_sent,
            stats_empty.bytes_sent
        );
    }

    /// With a 1 KB cap, an initiator with 500 events stops early.
    ///
    /// The clinic has the events and pushes them to the empty hub. The budget
    /// cap applies to the clinic's outgoing bytes, so it stops partway through.
    #[test]
    fn test_budget_cap_stops_early() {
        const EVENTS: usize = 500;
        let hub_dir = tempfile::tempdir().unwrap();
        let clinic_dir = tempfile::tempdir().unwrap();
        let hub_id = NodeId(30);
        let clinic_id = NodeId(31);

        // Events live on the clinic (initiator) -- the budget caps its outgoing sends.
        ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();
        {
            let mut eng =
                ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
            for i in 0..EVENTS {
                eng.submit(1, format!("budget-cap-evt-{i:0>40}").into_bytes())
                    .unwrap();
            }
            eng.sync().unwrap();
        }

        let (addr, hub_h) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);
        let mut transport = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        transport.connect(hub_id, &addr).unwrap();
        let stats = SyncSession::new(&mut eng, &mut transport)
            .with_max_bytes(1024)
            .sync(hub_id)
            .unwrap();
        hub_h.join().unwrap();

        assert!(
            stats.budget_exhausted,
            "1 KB cap with {EVENTS} events on initiator must exhaust the budget"
        );
        assert!(
            stats.events_sent < EVENTS,
            "fewer than {EVENTS} events must be sent under cap (sent {})",
            stats.events_sent
        );
    }

    /// Without a cap, all events are delivered and budget_exhausted is false.
    #[test]
    fn test_no_cap_delivers_all_events() {
        const EVENTS: usize = 100;
        let hub_dir = tempfile::tempdir().unwrap();
        let clinic_dir = tempfile::tempdir().unwrap();
        let hub_id = NodeId(40);
        let clinic_id = NodeId(41);

        {
            let mut eng = ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default()).unwrap();
            for i in 0..EVENTS {
                eng.submit(1, format!("nocap-evt-{i}").into_bytes())
                    .unwrap();
            }
            eng.sync().unwrap();
        }
        ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();

        let (addr, hub_h) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);
        let mut transport = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        transport.connect(hub_id, &addr).unwrap();
        let stats = SyncSession::new(&mut eng, &mut transport)
            .sync(hub_id)
            .unwrap();
        hub_h.join().unwrap();

        assert!(!stats.budget_exhausted, "no cap must not exhaust budget");
        assert_eq!(
            stats.events_received, EVENTS,
            "all {EVENTS} events must be received without a cap"
        );
    }

    /// A capped sync followed by an uncapped resume delivers all events with no
    /// re-transmission -- the VV picks up exactly where the first session stopped.
    ///
    /// Events live on the clinic (initiator). The budget caps how many events
    /// the clinic sends to the hub per session. After two syncs the hub holds
    /// all events; events_sent in the second sync is strictly less than EVENTS
    /// because the hub already has the first batch.
    #[test]
    fn test_capped_then_resume_delivers_all_events() {
        const EVENTS: usize = 300;
        let hub_dir = tempfile::tempdir().unwrap();
        let clinic_dir = tempfile::tempdir().unwrap();
        let hub_id = NodeId(50);
        let clinic_id = NodeId(51);

        // Clinic has all events; hub starts empty.
        ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default())
            .unwrap()
            .sync()
            .unwrap();
        {
            let mut eng =
                ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
            for i in 0..EVENTS {
                eng.submit(1, format!("resume-evt-{i:0>40}").into_bytes())
                    .unwrap();
            }
            eng.sync().unwrap();
        }

        // ---- First sync: 1 KB cap -----------------------------------------------
        let (addr1, hub_h1) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);
        let mut t1 = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut clinic_eng1 =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        t1.connect(hub_id, &addr1).unwrap();
        let stats1 = SyncSession::new(&mut clinic_eng1, &mut t1)
            .with_max_bytes(1024)
            .sync(hub_id)
            .unwrap();
        clinic_eng1.sync().unwrap();
        let hub_count_after_first = hub_h1.join().unwrap();

        assert!(
            stats1.budget_exhausted,
            "first sync must exhaust 1 KB budget"
        );
        assert!(
            hub_count_after_first < EVENTS,
            "hub must have received only a partial batch after capped sync (got {hub_count_after_first})"
        );

        // ---- Second sync: no cap (resume) ---------------------------------------
        let (addr2, hub_h2) = spawn_hub_once(hub_dir.path().to_path_buf(), hub_id);
        let mut t2 = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut clinic_eng2 =
            ZamEngine::open_wal(clinic_dir.path(), clinic_id, Counter::default()).unwrap();
        t2.connect(hub_id, &addr2).unwrap();
        let stats2 = SyncSession::new(&mut clinic_eng2, &mut t2)
            .sync(hub_id)
            .unwrap();
        clinic_eng2.sync().unwrap();
        let hub_count_after_second = hub_h2.join().unwrap();

        assert!(
            !stats2.budget_exhausted,
            "resume sync must not exhaust budget"
        );
        assert_eq!(
            hub_count_after_second, EVENTS,
            "hub must hold all {EVENTS} events after resume"
        );
        assert!(
            stats2.events_sent < EVENTS,
            "resume must not re-send events the hub already received (sent {})",
            stats2.events_sent
        );
        assert!(
            hub_count_after_second > hub_count_after_first,
            "resume must have delivered more events to the hub"
        );
    }

    // ---- concurrent hub --------------------------------------------------------

    /// Four clinic clients sync to a hub concurrently via `accept_split`.
    /// Each clinic submits 5 events offline. The hub must end up with all
    /// 20 events (4 clinics x 5 events). No deadlock, no data loss.
    #[test]
    fn test_concurrent_hub_four_clients() {
        const CLINICS: usize = 4;
        const EVENTS_PER_CLINIC: usize = 5;

        let hub_dir = tempfile::tempdir().unwrap();
        let hub_id = NodeId(1000);

        {
            let mut eng = ZamEngine::open_wal(hub_dir.path(), hub_id, Counter::default()).unwrap();
            eng.sync().unwrap();
        }

        let hub_path = hub_dir.path().to_path_buf();
        let mut hub_transport = TcpTransport::bind("127.0.0.1:0").unwrap();
        let hub_addr = hub_transport.local_addr().unwrap().to_string();

        // Barrier: all clients release simultaneously to exercise concurrent accept.
        let barrier = Arc::new(Barrier::new(CLINICS));

        let hub_thread = thread::spawn(move || {
            let mut handles = Vec::with_capacity(CLINICS);
            for _ in 0..CLINICS {
                let mut pt = hub_transport.accept_split().unwrap();
                let peer_id = pt.peer_id();
                let path = hub_path.clone();
                let h = thread::spawn(move || {
                    let mut eng = ZamEngine::open_wal(&path, hub_id, Counter::default()).unwrap();
                    SyncSession::new(&mut eng, &mut pt)
                        .serve_one(peer_id)
                        .unwrap();
                    eng.sync().unwrap();
                });
                handles.push(h);
            }
            for h in handles {
                h.join().unwrap();
            }
            let eng = ZamEngine::open_wal(&hub_path, hub_id, Counter::default()).unwrap();
            eng.state().count
        });

        let mut clinic_handles = Vec::with_capacity(CLINICS);
        for i in 0..CLINICS {
            let addr = hub_addr.clone();
            let bar = Arc::clone(&barrier);
            let h = thread::spawn(move || {
                let clinic_id = NodeId((i + 1) as u32);
                let dir = tempfile::tempdir().unwrap();
                let mut eng =
                    ZamEngine::open_wal(dir.path(), clinic_id, Counter::default()).unwrap();
                for j in 0..EVENTS_PER_CLINIC {
                    eng.submit(1, format!("clinic-{i}-evt-{j}").into_bytes())
                        .unwrap();
                }
                eng.sync().unwrap();

                bar.wait(); // release all clinics at once

                let mut transport = TcpTransport::bind("127.0.0.1:0").unwrap();
                transport.connect(NodeId(1000), &addr).unwrap();
                SyncSession::new(&mut eng, &mut transport)
                    .sync(NodeId(1000))
                    .unwrap();
            });
            clinic_handles.push(h);
        }

        for h in clinic_handles {
            h.join().unwrap();
        }

        let hub_event_count = hub_thread.join().unwrap();
        assert_eq!(
            hub_event_count,
            CLINICS * EVENTS_PER_CLINIC,
            "hub must hold all {CLINICS}x{EVENTS_PER_CLINIC} events after concurrent sync"
        );
    }
}
