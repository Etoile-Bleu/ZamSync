# ZamSync -- Resilient Offline-First Synchronization Engine

ZamSync is a systems-level synchronization engine designed for environments where network connectivity is unreliable, intermittent, or extremely low-bandwidth.

It targets real-world infrastructure constraints observed in rural and mountainous regions where traditional cloud-first or HTTP-based synchronization systems fail under network instability.

---

## Context and Motivation

Reliable digital infrastructure is still a major challenge in remote regions worldwide.

In countries with complex geography such as Bhutan, large parts of the territory are composed of high-altitude terrain, making connectivity inconsistent across villages, clinics, and administrative centers.

Public digital transformation initiatives such as national e-health systems face recurring challenges:

- unstable or intermittent connectivity
- high latency links (2G / satellite / constrained mobile networks)
- frequent disconnections during data synchronization
- fallback to manual or paper-based workflows during outages

---

## Design Goals

ZamSync is:

- offline-first by design
- resilient to frequent and unpredictable disconnections
- highly bandwidth-efficient
- deterministic and replay-safe
- domain-agnostic (no healthcare-specific logic inside the engine)

---

## Architecture

ZamSync uses **hexagonal architecture** (ports and adapters). The sync core is a set of pure Rust traits with no I/O. Storage and transport are pluggable adapters.

```
zamsync-core        -- Event, HLC, VersionVector, SyncMessage, port traits
zamsync-storage     -- WAL event store, file peer store, ZamEngine
zamsync-network     -- TCP transport, binary wire protocol
zamsync-testing     -- In-memory adapters, MockTransport, run_direct_sync
zamsync (binary)    -- CLI: info, submit
```

### Port Traits

```rust
// Implement StateStore to project events into your domain model
pub trait StateStore {
    fn apply_event(&mut self, seq: SequenceNumber, event: &Event) -> ZamResult<()>;
    fn last_applied_seq(&self) -> Option<SequenceNumber>;
}
```

The engine `ZamEngine<E: EventStore, P: PeerStore, S: StateStore>` is generic over all I/O. The WAL-backed stack is accessible via `ZamEngine::open_wal(data_dir, node_id, state)`.

### Sync Protocol

Peers exchange `SyncMessage` frames. A session:

1. Both sides send a `Handshake` carrying their Version Vector.
2. Each side computes gaps and pushes missing `EventBatch` messages.
3. Each side sends `SyncComplete` when done.

Events are idempotent: duplicate deliveries are dropped via VV check.

---

## Getting Started

### Build

```bash
cargo build --release
```

### CLI

```bash
# Show node status (creates data dir on first run)
./target/release/zamsync info /var/lib/zamsync/node1

# Submit an event
./target/release/zamsync submit /var/lib/zamsync/node1 "hello world"
```

Node identity is stored in `<data-dir>/.node_id` and generated automatically on first start.

### Using the Engine in Your Application

```rust
use zamsync_storage::ZamEngine;
use zamsync_core::{NodeId, ports::StateStore, Event, SequenceNumber, ZamResult};

struct MyState { /* ... */ }

impl StateStore for MyState {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        // project the event into your domain model
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> { None }
}

let mut engine = ZamEngine::open_wal("./data", NodeId(1), MyState { /* ... */ })?;
engine.submit(1, b"my payload".to_vec())?;
engine.sync()?; // flush WAL and persist replication state
```

---

## Testing

```bash
cargo test --workspace
```

The `zamsync-testing` crate provides in-memory adapters and `run_direct_sync` for convergence tests without any I/O.

---

## Failure Model

The system is explicitly designed to tolerate:

- frequent network disconnections
- high packet loss
- long offline periods
- partial transfers
- corrupted or incomplete WAL entries (detected via CRC32)

All sync operations are retry-safe and idempotent.

---

## Non-Goals

ZamSync explicitly avoids:

- blockchain-based consensus
- cloud-first architectural dependency
- semantic conflict resolution (this belongs in the StateStore)
- assumptions of stable connectivity

---

## Long-Term Vision

ZamSync is intended as a foundation for offline-first distributed systems in infrastructure-limited environments. The objective is to provide a predictable, deterministic, and robust synchronization engine that operates reliably where conventional systems fail.
