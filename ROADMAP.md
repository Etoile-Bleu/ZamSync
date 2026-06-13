# ZamSync Roadmap

## Phase 0: Foundation (done)

- [x] Workspace initialization and architecture documentation
- [x] Core type definitions: `NodeId`, `SequenceNumber`, `Hlc`
- [x] Production-grade WAL: atomic appends, CRC32 integrity, crash recovery
- [x] rkyv zero-copy event schema

## Phase 1: Hexagonal Architecture (done)

- [x] Port traits: `EventStore`, `PeerStore`, `StateStore`, `Transport`
- [x] Storage adapters: `WalEventStore`, `FilePeerStore`
- [x] Testing adapters: `InMemoryEventStore`, `InMemoryPeerStore`, `MockTransport`
- [x] `ZamEngine<E, P, S>` generic over all I/O
- [x] `ZamEngine::sorted_scan` -- deterministic multi-node replay via `LogSorter`
- [x] GitHub Actions CI: fmt + clippy -D warnings + test

## Phase 2: Sync Protocol (done)

- [x] Version Vectors with `find_gaps` (inclusive start_seq semantics)
- [x] HLC-based total ordering and `LogSorter` k-way merge
- [x] `SyncMessage` enum: Handshake, PullRequest, EventBatch, SyncComplete
- [x] `ZamEngine::handle_sync_message` -- server-side state machine
- [x] `SyncSession::sync` -- initiator-side protocol
- [x] `SyncSession::serve_one` -- responder-side protocol
- [x] `run_direct_sync` in `zamsync-testing` for transport-free tests
- [x] Idempotent `apply_replicated` with VV-based dedup

## Phase 3: Transport (done)

- [x] Binary wire protocol: 4-byte big-endian length prefix + rkyv payload
- [x] `TcpTransport`: non-blocking listener, `accept_peer`, `connect`
- [x] End-to-end TCP sync test: two nodes over loopback, full convergence verified
- [x] CLI: `info`, `submit`, `sync <peer-addr> <peer-id>`, `serve <bind-addr> <peer-id>`

## Phase 4: Hardening (done)

- [x] Crash-consistency test suite: WAL truncation, CRC corruption, VV recovery
- [x] Auto-truncate partial WAL writes on open (silent data-loss fix)
- [x] `serve` loop: continuous, auto-detects peer NodeId from Handshake
- [x] Structured logging: tracing spans per sync session, RUST_LOG filter
- [x] Reconnect and retry logic in `sync` CLI command (exponential backoff, 5 attempts)
- [x] Serve loop continues on peer errors instead of dying
- [x] Max frame size enforcement: 64 MB hard limit in wire protocol decoder

## Phase 5: Performance

- [x] Chunked `EventBatch`: 256 events/frame cap, multiple frames per sync (bounds frame size and peak memory)
- [x] WAL compaction: drop peer-confirmed events; tombstone record preserves seq continuity; `zamsync compact` CLI command
- [x] Zstd compression: level-3 on all frames >= 64 bytes, flag byte for decoder, transparent to all callers
- [ ] Resource profiling: target < 100 MB RSS on embedded hardware

## Phase 6: Security and Ops

- [ ] End-to-end encryption (noise protocol or TLS)
- [ ] Node authentication (pre-shared keys or certificate pinning)
- [x] Prometheus metrics: events_submitted, sync duration histogram, events_sent/received, VV drift gauge
- [ ] Docker image + systemd unit for unattended deployment

## First-Deployment Target

Bhutan ePIS (electronic patient information system):
- Clinics sync patient records over intermittent satellite / 2G links
- Nodes run on low-cost ARM hardware (Raspberry Pi class)
- Payload: structured JSON domain events, typically 1-10 KB each
- Acceptable sync latency: minutes to hours depending on connectivity
