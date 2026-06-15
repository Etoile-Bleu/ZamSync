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
    fn send(&mut self, _peer_id: NodeId, message: &SyncMessage) -> ZamResult<()> {
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
