use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zamsync_core::{Event, Hlc, NodeId, ReplicationState, SequenceNumber, ZamResult};
use zamsync_core::ports::{EventStore, PeerStore, StateStore};
use crate::adapters::{FilePeerStore, WalEventStore};

pub struct ZamEngine<E: EventStore, P: PeerStore, S: StateStore> {
    node_id: NodeId,
    event_store: E,
    peer_store: P,
    state: S,
    hlc: Hlc,
    replication: ReplicationState,
}

impl<E: EventStore, P: PeerStore, S: StateStore> ZamEngine<E, P, S> {
    pub fn new(
        node_id: NodeId,
        event_store: E,
        peer_store: P,
        mut state: S,
    ) -> ZamResult<Self> {
        let mut max_hlc = Hlc::default();

        for event_res in event_store.scan()? {
            let event = event_res?;
            if event.hlc > max_hlc {
                max_hlc = event.hlc;
            }
            state.apply_event(event.seq, &event)?;
        }

        let replication = peer_store.load()?;

        Ok(Self {
            node_id,
            event_store,
            peer_store,
            state,
            hlc: max_hlc,
            replication,
        })
    }

    pub fn submit(&mut self, event_type: u32, payload: Vec<u8>) -> ZamResult<SequenceNumber> {
        let now_ms = now_ms();
        self.hlc.tick(now_ms);
        let seq = self.event_store.next_seq();
        let event = Event {
            origin_node: self.node_id,
            seq,
            hlc: self.hlc,
            event_type,
            payload,
        };
        self.commit_event(event)
    }

    pub fn apply_replicated(&mut self, event: Event) -> ZamResult<SequenceNumber> {
        let now_ms = now_ms();
        self.hlc.sync(now_ms, &event.hlc);
        self.commit_event(event)
    }

    pub fn scan_events(&self) -> ZamResult<Box<dyn Iterator<Item = ZamResult<Event>>>> {
        self.event_store.scan()
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn replication_state(&self) -> &ReplicationState {
        &self.replication
    }

    pub fn sync(&mut self) -> ZamResult<()> {
        self.event_store.sync()?;
        self.peer_store.save(&self.replication)
    }

    fn commit_event(&mut self, event: Event) -> ZamResult<SequenceNumber> {
        let local_seq = self.event_store.append(&event)?;
        self.state.apply_event(local_seq, &event)?;
        self.replication.local_vv.update(event.origin_node, event.seq);
        Ok(local_seq)
    }
}

impl<S: StateStore> ZamEngine<WalEventStore, FilePeerStore, S> {
    pub fn open_wal(
        data_dir: impl AsRef<Path>,
        node_id: NodeId,
        state: S,
    ) -> ZamResult<Self> {
        let dir = data_dir.as_ref();
        let event_store = WalEventStore::open(dir.join("events.wal"))?;
        let peer_store = FilePeerStore::open(dir.join("peers.state"), node_id)?;
        ZamEngine::new(node_id, event_store, peer_store, state)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
