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
    /// Bytes written to the wire by the local node during this session.
    pub bytes_sent: u64,
    /// True when a `--max-bytes` cap was reached and the session ended early.
    pub budget_exhausted: bool,
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
    /// Hard cap on bytes the initiator sends in one session (EventBatches +
    /// control frames). When set, the session sends `SyncComplete` early once
    /// the budget is reached, and resumes from the correct VV on the next run.
    max_bytes: Option<u64>,
}

impl<'a, E, P, S, T> SyncSession<'a, E, P, S, T>
where
    E: EventStore,
    P: PeerStore,
    S: StateStore,
    T: Transport,
{
    pub fn new(engine: &'a mut ZamEngine<E, P, S>, transport: &'a mut T) -> Self {
        Self {
            engine,
            transport,
            max_bytes: None,
        }
    }

    /// Cap the bytes this initiator session is allowed to send on the wire.
    ///
    /// The limit applies to the cumulative wire bytes of all frames sent
    /// (control + data). When the budget is reached before all gaps are filled,
    /// the session sends `SyncComplete` and returns with `budget_exhausted =
    /// true`. The peer's VV already reflects the last applied batch, so the
    /// next invocation resumes without re-transmitting anything.
    pub fn with_max_bytes(mut self, limit: u64) -> Self {
        self.max_bytes = Some(limit);
        self
    }

    /// Initiator side: sends our handshake, receives peer's handshake + events +
    /// SyncComplete, then pushes our missing events and sends SyncComplete.
    #[instrument(skip(self), fields(peer = peer_id.0))]
    pub fn sync(&mut self, peer_id: NodeId) -> ZamResult<SyncStats> {
        let t0 = Instant::now();
        let peer_label = peer_id.0.to_string();
        let mut stats = SyncStats::default();

        let n = self
            .transport
            .send(peer_id, &self.engine.prepare_handshake())?;
        stats.bytes_sent += n as u64;

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

        // Push events the peer is missing, stopping early if the byte budget is reached.
        let gaps = peer_vv.find_gaps(&our_vv);
        'send_loop: for (node, start_seq) in gaps {
            let events = self.engine.events_since(node, start_seq)?;
            for chunk in events.chunks(EVENTS_PER_BATCH) {
                // Check budget *before* sending the next batch so bytes_sent
                // never exceeds max_bytes + one_batch_overhead.
                if let Some(max) = self.max_bytes {
                    if stats.bytes_sent >= max {
                        stats.budget_exhausted = true;
                        break 'send_loop;
                    }
                }
                let n = self.transport.send(
                    peer_id,
                    &SyncMessage::EventBatch {
                        origin_node: node,
                        events: chunk.to_vec(),
                    },
                )?;
                stats.events_sent += chunk.len();
                stats.bytes_sent += n as u64;
            }
        }

        let n = self.transport.send(peer_id, &SyncMessage::SyncComplete)?;
        stats.bytes_sent += n as u64;

        // Wait for the responder to process everything and close gracefully.
        // This prevents the initiator from exiting and resetting the socket
        // before the responder has flushed its writes.
        loop {
            match self.transport.receive() {
                Ok(None) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Ok(Some(_)) => continue,
                Err(zamsync_core::ZamError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        // Emit metrics
        counter!("zamsync_sync_events_sent_total", "peer" => peer_label.clone())
            .increment(stats.events_sent as u64);
        counter!("zamsync_sync_events_received_total", "peer" => peer_label.clone())
            .increment(stats.events_received as u64);
        counter!("zamsync_bytes_sent_total", "peer" => peer_label.clone())
            .increment(stats.bytes_sent);
        if stats.budget_exhausted {
            counter!("zamsync_budget_exhausted_total", "peer" => peer_label.clone()).increment(1);
        }
        histogram!("zamsync_sync_duration_seconds", "role" => "initiator")
            .record(t0.elapsed().as_secs_f64());

        tracing::info!(
            peer = peer_id.0,
            sent = stats.events_sent,
            received = stats.events_received,
            bytes_sent = stats.bytes_sent,
            budget_exhausted = stats.budget_exhausted,
            "sync complete"
        );
        self.engine.sync()?;
        gauge!("zamsync_wal_size_bytes").set(self.engine.wal_byte_size() as f64);
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
                        let n = self.transport.send(peer_id, response)?;
                        stats.bytes_sent += n as u64;
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
        counter!("zamsync_bytes_sent_total", "peer" => peer_label.clone())
            .increment(stats.bytes_sent);
        histogram!("zamsync_sync_duration_seconds", "role" => "responder")
            .record(t0.elapsed().as_secs_f64());

        tracing::info!(
            peer = peer_id.0,
            sent = stats.events_sent,
            received = stats.events_received,
            bytes_sent = stats.bytes_sent,
            "serve_one complete"
        );
        self.engine.sync()?;
        gauge!("zamsync_wal_size_bytes").set(self.engine.wal_byte_size() as f64);
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
