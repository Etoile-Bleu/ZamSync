use std::collections::HashMap;
use rkyv::{Archive, Deserialize, Serialize};
use crate::{NodeId, SequenceNumber};

/// State of synchronization with a specific peer.
#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default)]
#[archive(check_bytes)]
pub struct PeerSyncState {
    /// The last sequence number we received from this peer.
    pub last_received: Option<SequenceNumber>,
    /// The last sequence number of OURS that this peer has acknowledged.
    pub last_acked: Option<SequenceNumber>,
}

/// A collection of sync states for all known peers.
#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default)]
#[archive(check_bytes)]
pub struct ReplicationState {
    pub self_id: NodeId,
    pub peers: HashMap<u32, PeerSyncState>,
}
