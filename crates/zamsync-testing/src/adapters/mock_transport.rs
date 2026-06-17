use std::collections::VecDeque;
use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, ZamResult};

pub struct MockTransport {
    outbox: Vec<(NodeId, SyncMessage)>,
    inbox: VecDeque<(NodeId, SyncMessage)>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            outbox: Vec::new(),
            inbox: VecDeque::new(),
        }
    }

    pub fn inject(&mut self, from: NodeId, message: SyncMessage) {
        self.inbox.push_back((from, message));
    }

    pub fn drain_outbox(&mut self) -> Vec<(NodeId, SyncMessage)> {
        std::mem::take(&mut self.outbox)
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockTransport {
    /// Stores the message in the outbox and returns a fixed per-message byte
    /// estimate. The estimate is intentionally rough -- MockTransport is for
    /// routing tests, not wire-byte accuracy. Use a real TcpTransport when
    /// byte-level accounting matters.
    fn send(&mut self, peer_id: NodeId, message: &SyncMessage) -> ZamResult<usize> {
        self.outbox.push((peer_id, message.clone()));
        // 32-byte baseline per frame (header + control overhead)
        let data_bytes = match message {
            SyncMessage::EventBatch { events, .. } => events.len() * 64,
            _ => 0,
        };
        Ok(32 + data_bytes)
    }

    fn receive(&mut self) -> ZamResult<Option<(NodeId, SyncMessage)>> {
        Ok(self.inbox.pop_front())
    }
}
