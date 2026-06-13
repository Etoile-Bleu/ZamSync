use zamsync_core::{NodeId, SequenceNumber, VersionVector};
use zamsync_core::ports::PeerStore;
use zamsync_storage::FilePeerStore;
use tempfile::tempdir;

#[test]
fn test_version_vector_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let state_path = dir.path().join("peers.state");
    let self_id = NodeId(1);
    let peer_id = NodeId(2);
    let node_c = NodeId(3);

    {
        let mut store = FilePeerStore::open(&state_path, self_id)?;
        let mut state = store.load()?;
        state.local_vv.update(self_id, SequenceNumber(10));
        state.local_vv.update(node_c, SequenceNumber(5));
        let mut peer_vv = VersionVector::new();
        peer_vv.update(self_id, SequenceNumber(8));
        state.peers.entry(peer_id.0).or_default().known_vv = peer_vv;
        store.save(&state)?;
    }

    {
        let store = FilePeerStore::open(&state_path, self_id)?;
        let state = store.load()?;
        assert_eq!(state.local_vv.get(self_id), SequenceNumber(10));
        assert_eq!(state.local_vv.get(node_c), SequenceNumber(5));
        let peer_vv = &state.peers[&peer_id.0].known_vv;
        assert_eq!(peer_vv.get(self_id), SequenceNumber(8));
        assert_eq!(peer_vv.get(NodeId(99)), SequenceNumber(0));
    }

    Ok(())
}

#[test]
fn test_vv_gap_discovery() {
    let mut local = VersionVector::new();
    local.update(NodeId(1), SequenceNumber(10));
    local.update(NodeId(2), SequenceNumber(5));

    let mut remote = VersionVector::new();
    remote.update(NodeId(1), SequenceNumber(12));
    remote.update(NodeId(2), SequenceNumber(5));
    remote.update(NodeId(3), SequenceNumber(3));

    let gaps = local.find_gaps(&remote);
    assert_eq!(gaps.len(), 2);
    assert!(gaps.iter().any(|(id, seq)| id.0 == 1 && seq.0 == 10));
    assert!(gaps.iter().any(|(id, seq)| id.0 == 3 && seq.0 == 0));
}
