use crate::protocol;
use std::collections::HashMap;
use std::io::BufWriter;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;
use tracing::{info, warn};
use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, ZamError, ZamResult};

struct PeerConn {
    stream: TcpStream,
    /// One message read ahead (e.g. Handshake consumed during accept_any).
    pending: Option<SyncMessage>,
}

pub struct TcpTransport {
    listener: TcpListener,
    peers: HashMap<u32, PeerConn>,
}

impl TcpTransport {
    pub fn bind(addr: &str) -> ZamResult<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        info!("listening on {}", addr);
        Ok(Self {
            listener,
            peers: HashMap::new(),
        })
    }

    pub fn local_addr(&self) -> ZamResult<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Blocking accept: waits for one incoming connection and registers it as `peer_id`.
    pub fn accept_peer(&mut self, peer_id: NodeId) -> ZamResult<()> {
        self.listener.set_nonblocking(false)?;
        let (stream, addr) = self.listener.accept()?;
        self.listener.set_nonblocking(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        self.peers.insert(
            peer_id.0,
            PeerConn {
                stream,
                pending: None,
            },
        );
        info!(peer = peer_id.0, %addr, "accepted peer");
        Ok(())
    }

    /// Blocking accept: reads the first Handshake to discover the peer's NodeId.
    /// The Handshake is buffered and will be returned by the next `receive()` call.
    /// Returns the peer's NodeId so callers can pass it to `serve_one`.
    pub fn accept_any(&mut self) -> ZamResult<NodeId> {
        self.listener.set_nonblocking(false)?;
        let (mut stream, addr) = self.listener.accept()?;
        self.listener.set_nonblocking(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(5_000)))?;

        let msg = protocol::decode(&mut stream)?;
        let node_id = match &msg {
            SyncMessage::Handshake { node_id, .. } => *node_id,
            other => {
                warn!(
                    ?other,
                    "expected Handshake as first message, closing connection"
                );
                return Err(ZamError::Protocol(
                    "first message from peer must be a Handshake".into(),
                ));
            }
        };

        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        self.peers.insert(
            node_id.0,
            PeerConn {
                stream,
                pending: Some(msg),
            },
        );
        info!(peer = node_id.0, %addr, "accepted peer via Handshake");
        Ok(node_id)
    }

    /// Removes a peer connection, allowing the slot to be reused.
    pub fn disconnect(&mut self, peer_id: NodeId) {
        self.peers.remove(&peer_id.0);
    }

    pub fn connect(&mut self, peer_id: NodeId, addr: &str) -> ZamResult<()> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        self.peers.insert(
            peer_id.0,
            PeerConn {
                stream,
                pending: None,
            },
        );
        info!(peer = peer_id.0, addr, "connected to peer");
        Ok(())
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, peer_id: NodeId, message: &SyncMessage) -> ZamResult<()> {
        let peer = self
            .peers
            .get_mut(&peer_id.0)
            .ok_or_else(|| ZamError::Protocol(format!("no connection to peer {}", peer_id.0)))?;
        let mut writer = BufWriter::new(&peer.stream);
        protocol::encode(message, &mut writer)
    }

    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>> {
        let peer_ids: Vec<u32> = self.peers.keys().cloned().collect();
        for peer_id_raw in peer_ids {
            if let Some(peer) = self.peers.get_mut(&peer_id_raw) {
                if let Some(msg) = peer.pending.take() {
                    return Ok(Some((NodeId(peer_id_raw), msg)));
                }
                match protocol::decode(&mut peer.stream) {
                    Ok(msg) => return Ok(Some((NodeId(peer_id_raw), msg))),
                    Err(ZamError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Full bidirectional sync over real TCP loopback:
    /// - A submits 3 events, B submits 2 events before any sync
    /// - A initiates sync to B (A is initiator, B serves one session)
    /// - After sync both nodes must have all 5 events
    #[test]
    fn test_tcp_two_node_full_sync() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let node_a = NodeId(1);
        let node_b = NodeId(2);

        // Pre-populate both nodes before sync
        {
            let mut eng = ZamEngine::open_wal(dir_a.path(), node_a, Counter::default()).unwrap();
            eng.submit(1, b"a-evt-1".to_vec()).unwrap();
            eng.submit(1, b"a-evt-2".to_vec()).unwrap();
            eng.submit(1, b"a-evt-3".to_vec()).unwrap();
            eng.sync().unwrap();
        }
        {
            let mut eng = ZamEngine::open_wal(dir_b.path(), node_b, Counter::default()).unwrap();
            eng.submit(1, b"b-evt-1".to_vec()).unwrap();
            eng.submit(1, b"b-evt-2".to_vec()).unwrap();
            eng.sync().unwrap();
        }

        // B listens
        let mut transport_b = TcpTransport::bind("127.0.0.1:0").unwrap();
        let b_addr = transport_b.local_addr().unwrap().to_string();
        let path_b = dir_b.path().to_path_buf();

        let b_thread = thread::spawn(move || {
            let mut eng = ZamEngine::open_wal(&path_b, node_b, Counter::default()).unwrap();
            let peer_id = transport_b.accept_any().unwrap();
            SyncSession::new(&mut eng, &mut transport_b)
                .serve_one(peer_id)
                .unwrap();
            eng.sync().unwrap();
            eng.state().count
        });

        // A connects and initiates sync
        let mut transport_a = TcpTransport::bind("127.0.0.1:0").unwrap();
        let mut eng_a = ZamEngine::open_wal(dir_a.path(), node_a, Counter::default()).unwrap();
        transport_a.connect(node_b, &b_addr).unwrap();
        let stats = SyncSession::new(&mut eng_a, &mut transport_a)
            .sync(node_b)
            .unwrap();
        eng_a.sync().unwrap();

        let b_count = b_thread.join().unwrap();

        assert_eq!(eng_a.state().count, 5, "A should have all 5 events after sync");
        assert_eq!(b_count, 5, "B should have all 5 events after sync");
        assert_eq!(stats.events_sent, 3, "A sent its 3 events to B");
        assert_eq!(stats.events_received, 2, "A received B's 2 events");
    }
}
