use metrics::{counter, gauge, histogram};
use std::time::Instant;
use tracing::{instrument, warn};
use zamsync_core::ports::{EventStore, PeerStore, StateStore, Transport};
use zamsync_core::{NodeId, SequenceNumber, SyncMessage, ZamError, ZamResult};

use crate::engine::{ZamEngine, EVENTS_PER_BATCH};

#[derive(Debug, Default)]
pub struct SyncStats {
    pub events_sent: usize,
    pub events_received: usize,
}

pub struct SyncSession<'a, E, P, S, T>
where
    E: EventStore,
    P: PeerStore,
    S: StateStore,
    T: Transport,
{
    engine: &'a mut ZamEngine<E, P, S>,
    transport: &'a mut T,
}

impl<'a, E, P, S, T> SyncSession<'a, E, P, S, T>
where
    E: EventStore,
    P: PeerStore,
    S: StateStore,
    T: Transport,
{
    pub fn new(engine: &'a mut ZamEngine<E, P, S>, transport: &'a mut T) -> Self {
        Self { engine, transport }
    }

    /// Initiator side: sends our handshake, receives peer's handshake + events +
    /// SyncComplete, then pushes our missing events and sends SyncComplete.
    #[instrument(skip(self), fields(peer = peer_id.0))]
    pub fn sync(&mut self, peer_id: NodeId) -> ZamResult<SyncStats> {
        let t0 = Instant::now();
        let peer_label = peer_id.0.to_string();
        let mut stats = SyncStats::default();

        self.transport
            .send(peer_id, &self.engine.prepare_handshake())?;

        let peer_vv = self.wait_for_handshake(peer_id)?;

        // Compute how many events peer needs from us (VV drift from initiator's view).
        let our_vv = self.engine.replication_state().local_vv.clone();
        let drift: u64 = our_vv
            .entries
            .iter()
            .map(|(node, our_seq)| {
                let peer_seq = peer_vv
                    .entries
                    .get(node)
                    .copied()
                    .unwrap_or(SequenceNumber::ZERO);
                our_seq.0.saturating_sub(peer_seq.0)
            })
            .sum();
        gauge!("zamsync_vv_drift_events", "peer" => peer_label.clone()).set(drift as f64);

        loop {
            match self.transport.receive()? {
                Some((from, msg)) if from == peer_id => {
                    let is_complete = matches!(msg, SyncMessage::SyncComplete);
                    if let SyncMessage::EventBatch { ref events, .. } = msg {
                        stats.events_received += events.len();
                    }
                    self.engine.handle_sync_message(from, msg)?;
                    if is_complete {
                        break;
                    }
                }
                Some(_) => continue,
                None => continue,
            }
        }

        let gaps = peer_vv.find_gaps(&our_vv);
        for (node, start_seq) in gaps {
            let events = self.engine.events_since(node, start_seq)?;
            for chunk in events.chunks(EVENTS_PER_BATCH) {
                stats.events_sent += chunk.len();
                self.transport.send(
                    peer_id,
                    &SyncMessage::EventBatch {
                        origin_node: node,
                        events: chunk.to_vec(),
                    },
                )?;
            }
        }
        self.transport.send(peer_id, &SyncMessage::SyncComplete)?;

        // Emit metrics
        counter!("zamsync_sync_events_sent_total", "peer" => peer_label.clone())
            .increment(stats.events_sent as u64);
        counter!("zamsync_sync_events_received_total", "peer" => peer_label.clone())
            .increment(stats.events_received as u64);
        histogram!("zamsync_sync_duration_seconds", "role" => "initiator")
            .record(t0.elapsed().as_secs_f64());

        tracing::info!(
            peer = peer_id.0,
            sent = stats.events_sent,
            received = stats.events_received,
            "sync complete"
        );
        self.engine.sync()?;
        Ok(stats)
    }

    /// Responder side: waits for the initiator's handshake, responds with our
    /// handshake + events + SyncComplete, then receives initiator's events until
    /// their SyncComplete.
    #[instrument(skip(self), fields(peer = peer_id.0))]
    pub fn serve_one(&mut self, peer_id: NodeId) -> ZamResult<SyncStats> {
        let t0 = Instant::now();
        let peer_label = peer_id.0.to_string();
        let mut stats = SyncStats::default();

        // Phase 1: wait for initiator's Handshake, respond immediately
        loop {
            match self.transport.receive()? {
                Some((from, msg @ SyncMessage::Handshake { .. })) if from == peer_id => {
                    // Compute VV drift before consuming the message.
                    if let SyncMessage::Handshake { ref vv, .. } = msg {
                        let our_vv = self.engine.replication_state().local_vv.clone();
                        let drift: u64 = our_vv
                            .entries
                            .iter()
                            .map(|(node, our_seq)| {
                                let peer_seq = vv
                                    .entries
                                    .get(node)
                                    .copied()
                                    .unwrap_or(SequenceNumber::ZERO);
                                our_seq.0.saturating_sub(peer_seq.0)
                            })
                            .sum();
                        gauge!("zamsync_vv_drift_events", "peer" => peer_label.clone())
                            .set(drift as f64);
                    }

                    let responses = self.engine.handle_sync_message(from, msg)?;
                    for response in &responses {
                        if let SyncMessage::EventBatch { events, .. } = response {
                            stats.events_sent += events.len();
                        }
                        self.transport.send(peer_id, response)?;
                    }
                    break;
                }
                Some(_) | None => continue,
            }
        }

        // Phase 2: receive initiator's events until their SyncComplete
        loop {
            match self.transport.receive()? {
                Some((from, msg)) if from == peer_id => {
                    let is_complete = matches!(msg, SyncMessage::SyncComplete);
                    if let SyncMessage::EventBatch { ref events, .. } = msg {
                        stats.events_received += events.len();
                    }
                    self.engine.handle_sync_message(from, msg)?;
                    if is_complete {
                        break;
                    }
                }
                Some(_) | None => continue,
            }
        }

        // Emit metrics
        counter!("zamsync_sync_events_sent_total", "peer" => peer_label.clone())
            .increment(stats.events_sent as u64);
        counter!("zamsync_sync_events_received_total", "peer" => peer_label.clone())
            .increment(stats.events_received as u64);
        histogram!("zamsync_sync_duration_seconds", "role" => "responder")
            .record(t0.elapsed().as_secs_f64());

        tracing::info!(
            peer = peer_id.0,
            sent = stats.events_sent,
            received = stats.events_received,
            "serve_one complete"
        );
        self.engine.sync()?;
        Ok(stats)
    }

    fn wait_for_handshake(
        &mut self,
        expected_peer: NodeId,
    ) -> ZamResult<zamsync_core::VersionVector> {
        for _ in 0..10_000 {
            match self.transport.receive()? {
                Some((from, SyncMessage::Handshake { vv, .. })) if from == expected_peer => {
                    return Ok(vv);
                }
                Some(_) | None => continue,
            }
        }
        warn!(peer = expected_peer.0, "timeout waiting for peer handshake");
        Err(ZamError::Protocol(
            "timeout waiting for peer handshake".into(),
        ))
    }
}
