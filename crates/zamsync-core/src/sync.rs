use std::collections::HashMap;
use rkyv::{Archive, Deserialize, Serialize};
use crate::{Event, NodeId, SequenceNumber};

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default, PartialEq, Eq)]
#[archive(check_bytes)]
pub struct VersionVector {
    pub entries: HashMap<u32, SequenceNumber>,
}

impl VersionVector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, node_id: NodeId, seq: SequenceNumber) {
        let entry = self.entries.entry(node_id.0).or_insert(SequenceNumber::ZERO);
        if seq > *entry {
            *entry = seq;
        }
    }

    pub fn get(&self, node_id: NodeId) -> SequenceNumber {
        self.entries.get(&node_id.0).cloned().unwrap_or(SequenceNumber::ZERO)
    }

    pub fn find_gaps(&self, other: &VersionVector) -> Vec<(NodeId, SequenceNumber)> {
        let mut gaps = Vec::new();
        for (node_id_raw, other_seq) in &other.entries {
            let node_id = NodeId(*node_id_raw);
            let local_seq = self.get(node_id);
            if *other_seq > local_seq {
                gaps.push((node_id, local_seq));
            }
        }
        gaps
    }
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default)]
#[archive(check_bytes)]
pub struct PeerSyncState {
    pub known_vv: VersionVector,
    pub last_acked: Option<SequenceNumber>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default)]
#[archive(check_bytes)]
pub struct ReplicationState {
    pub self_id: NodeId,
    pub local_vv: VersionVector,
    pub peers: HashMap<u32, PeerSyncState>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[archive(check_bytes)]
pub enum SyncMessage {
    Handshake {
        node_id: NodeId,
        vv: VersionVector,
    },
    PullRequest {
        origin_node: NodeId,
        start_seq: SequenceNumber,
        limit: u32,
    },
    EventBatch {
        origin_node: NodeId,
        events: Vec<Event>,
    },
}
