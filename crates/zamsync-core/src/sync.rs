use crate::{Event, NodeId, SequenceNumber};
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashMap;

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
        let entry = self
            .entries
            .entry(node_id.0)
            .or_insert(SequenceNumber::ZERO);
        if seq > *entry {
            *entry = seq;
        }
    }

    pub fn get(&self, node_id: NodeId) -> SequenceNumber {
        self.entries
            .get(&node_id.0)
            .cloned()
            .unwrap_or(SequenceNumber::ZERO)
    }

    /// Returns the first sequence number this VV needs from `other`, for each node where
    /// `other` is ahead. The returned `SequenceNumber` is the inclusive start of the gap
    /// (i.e. `events_since(node, start)` should return events with `seq >= start`).
    pub fn find_gaps(&self, other: &VersionVector) -> Vec<(NodeId, SequenceNumber)> {
        let mut gaps = Vec::new();
        for (node_id_raw, other_seq) in &other.entries {
            let node_id = NodeId(*node_id_raw);
            match self.entries.get(node_id_raw) {
                Some(&local_last) if *other_seq > local_last => {
                    gaps.push((node_id, local_last.next()));
                }
                None => {
                    gaps.push((node_id, SequenceNumber::ZERO));
                }
                _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_vv_update_only_advances() {
        let mut vv = VersionVector::new();
        vv.update(NodeId(1), SequenceNumber(5));
        assert_eq!(vv.get(NodeId(1)), SequenceNumber(5));
        // Lower seq must not overwrite a higher one.
        vv.update(NodeId(1), SequenceNumber(3));
        assert_eq!(vv.get(NodeId(1)), SequenceNumber(5), "VV must never decrease");
        // Same seq is idempotent.
        vv.update(NodeId(1), SequenceNumber(5));
        assert_eq!(vv.get(NodeId(1)), SequenceNumber(5));
        // Higher seq advances.
        vv.update(NodeId(1), SequenceNumber(10));
        assert_eq!(vv.get(NodeId(1)), SequenceNumber(10));
    }

    #[test]
    fn test_vv_get_unknown_node_returns_zero() {
        let vv = VersionVector::new();
        assert_eq!(vv.get(NodeId(42)), SequenceNumber::ZERO);
    }

    #[test]
    fn test_vv_find_gaps_empty_local_needs_everything_from_zero() {
        let local = VersionVector::new();
        let mut remote = VersionVector::new();
        remote.update(NodeId(1), SequenceNumber(10));
        remote.update(NodeId(2), SequenceNumber(5));

        let gaps: HashMap<u32, SequenceNumber> =
            local.find_gaps(&remote).into_iter().map(|(n, s)| (n.0, s)).collect();
        assert_eq!(gaps[&1], SequenceNumber::ZERO, "unknown node: start from seq 0");
        assert_eq!(gaps[&2], SequenceNumber::ZERO);
    }

    #[test]
    fn test_vv_find_gaps_partial_overlap_returns_next_needed() {
        let mut local = VersionVector::new();
        local.update(NodeId(1), SequenceNumber(5));

        let mut remote = VersionVector::new();
        remote.update(NodeId(1), SequenceNumber(10));
        remote.update(NodeId(2), SequenceNumber(3));

        let gaps: HashMap<u32, SequenceNumber> =
            local.find_gaps(&remote).into_iter().map(|(n, s)| (n.0, s)).collect();
        // local has node 1 up to seq 5, remote has 10 → need seq 6 (local.next())
        assert_eq!(gaps[&1], SequenceNumber(6));
        // node 2 is unknown to local → need from seq 0
        assert_eq!(gaps[&2], SequenceNumber::ZERO);
    }

    #[test]
    fn test_vv_find_gaps_up_to_date_returns_empty() {
        let mut local = VersionVector::new();
        local.update(NodeId(1), SequenceNumber(10));

        let mut remote = VersionVector::new();
        remote.update(NodeId(1), SequenceNumber(10));

        assert!(local.find_gaps(&remote).is_empty(), "no gap when equal");
    }

    #[test]
    fn test_vv_find_gaps_local_ahead_returns_empty() {
        let mut local = VersionVector::new();
        local.update(NodeId(1), SequenceNumber(15));

        let mut remote = VersionVector::new();
        remote.update(NodeId(1), SequenceNumber(10));

        assert!(
            local.find_gaps(&remote).is_empty(),
            "local is ahead, nothing to pull"
        );
    }

    #[test]
    fn test_vv_find_gaps_start_is_inclusive_next_seq() {
        let mut local = VersionVector::new();
        local.update(NodeId(1), SequenceNumber(3));

        let mut remote = VersionVector::new();
        remote.update(NodeId(1), SequenceNumber(7));

        let gaps = local.find_gaps(&remote);
        assert_eq!(gaps.len(), 1);
        // local has up to seq 3, remote has 7 → first missing is seq 4 (3.next())
        assert_eq!(gaps[0], (NodeId(1), SequenceNumber(4)));
    }

    #[test]
    fn test_vv_find_gaps_ignores_nodes_not_in_remote() {
        let mut local = VersionVector::new();
        local.update(NodeId(1), SequenceNumber(5));

        // remote is empty -- local has events remote doesn't, but that's not a "gap"
        // (gaps are defined as what the remote has that we don't)
        let remote = VersionVector::new();
        assert!(local.find_gaps(&remote).is_empty());
    }
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
    SyncComplete,
}
