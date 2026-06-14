use crate::protocol;
use std::collections::HashMap;
use std::io::BufWriter;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;
use tracing::{info, warn};
use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, ZamError, ZamResult};

use super::peer::TcpPeerTransport;

// ---- Internal per-connection state ------------------------------------------

struct PeerConn {
    stream: TcpStream,
    frame_buf: protocol::FrameBuffer,
    pending: Option<SyncMessage>,
}

impl PeerConn {
    fn new(stream: TcpStream, pending: Option<SyncMessage>) -> Self {
        Self {
            stream,
            frame_buf: protocol::FrameBuffer::new(),
            pending,
        }
    }
}

// ---- TcpTransport -----------------------------------------------------------

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

    /// Temporarily switches the listener to blocking mode, accepts one TCP
    /// connection, then restores non-blocking mode.
    fn raw_accept(&mut self) -> ZamResult<(TcpStream, SocketAddr)> {
        self.listener.set_nonblocking(false)?;
        let result = self.listener.accept()?;
        self.listener.set_nonblocking(true)?;
        Ok(result)
    }

    /// Blocking accept: waits for one incoming connection and registers it as `peer_id`.
    pub fn accept_peer(&mut self, peer_id: NodeId) -> ZamResult<()> {
        let (stream, addr) = self.raw_accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        self.peers.insert(peer_id.0, PeerConn::new(stream, None));
        info!(peer = peer_id.0, %addr, "accepted peer");
        Ok(())
    }

    /// Blocking accept: reads the first Handshake to discover the peer's NodeId.
    /// The Handshake is buffered and returned by the next `receive()` call.
    pub fn accept_any(&mut self) -> ZamResult<NodeId> {
        let (mut stream, addr) = self.raw_accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(5_000)))?;

        let msg = protocol::decode(&mut stream)?;
        let node_id = match &msg {
            SyncMessage::Handshake { node_id, .. } => *node_id,
            other => {
                warn!(?other, "expected Handshake as first message, closing connection");
                return Err(ZamError::Protocol(
                    "first message from peer must be a Handshake".into(),
                ));
            }
        };

        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        self.peers.insert(node_id.0, PeerConn::new(stream, Some(msg)));
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
        self.peers.insert(peer_id.0, PeerConn::new(stream, None));
        info!(peer = peer_id.0, addr, "connected to peer");
        Ok(())
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Accepts one connection and returns a self-contained per-peer transport.
    ///
    /// Unlike [`accept_any`], the connection is not stored in the internal
    /// HashMap. The returned [`TcpPeerTransport`] is `Send` and can be moved
    /// into a worker thread for concurrent hub serving.
    pub fn accept_split(&mut self) -> ZamResult<TcpPeerTransport> {
        let (mut stream, addr) = self.raw_accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(5_000)))?;

        let msg = protocol::decode(&mut stream)?;
        let node_id = match &msg {
            SyncMessage::Handshake { node_id, .. } => *node_id,
            other => {
                warn!(?other, "expected Handshake, closing connection");
                return Err(ZamError::Protocol(
                    "first message from peer must be a Handshake".into(),
                ));
            }
        };

        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        info!(peer = node_id.0, %addr, "accepted peer (split mode)");
        Ok(TcpPeerTransport::new(node_id, stream, Some(msg)))
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
                match peer.frame_buf.try_read_frame(&mut peer.stream) {
                    Ok(Some(bytes)) => {
                        let msg = rkyv::from_bytes::<SyncMessage>(&bytes)
                            .map_err(|e| ZamError::Serialization(format!("{}", e)))?;
                        return Ok(Some((NodeId(peer_id_raw), msg)));
                    }
                    Ok(None) => continue,
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

    /// Full bidirectional sync over real TCP loopback.
    /// A submits 3 events, B submits 2; after sync both nodes hold all 5.
    #[test]
    fn test_tcp_two_node_full_sync() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let node_a = NodeId(1);
        let node_b = NodeId(2);

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
