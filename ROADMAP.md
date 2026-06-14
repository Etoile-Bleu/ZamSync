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
- [x] Resource profiling: `zamsync bench <data-dir> [--events N]` -- 321k events/sec on x86, ~125 bytes/WAL record, RSS reported via `/proc/self/status` on Linux ARM

## Phase 6: Security and Ops

- [x] End-to-end encryption: mutual TLS (mTLS) with rustls (pure Rust, ARM-compatible)
- [x] Node authentication: certificate-based via shared CA; unauthorized nodes rejected at TLS handshake
- [x] `zamsync keygen <data-dir>` -- generates CA + node cert pair + WAL encryption key (`data.key`)
- [x] Prometheus metrics: events_submitted, sync duration histogram, events_sent/received, VV drift gauge
- [x] Docker image + systemd unit for unattended deployment (ARM64/ARMv7 via `docker buildx`)
- [x] WAL encryption at rest: ChaCha20-Poly1305 AEAD, random nonce per record, `--key-file` flag on all commands

## Phase 7: Compliance and Access Control

- [x] Audit trail: `zamsync audit <data-dir>` -- immutable per-event log with ISO 8601 timestamp, origin node, seq, type, payload size, SHA-256 integrity hash; JSON Lines (`--format json`) and text output; filter by `--since <unix-ms>` and `--node <id>`; `--key-file` for encrypted WALs
- [x] Payload schema validation: `--schema none|json|json+field1,field2` on all write commands; validates at `submit()` and `apply_replicated()`; prevents malformed events from entering or propagating through the WAL
- [x] Access control: `--policy own` on `serve`; hub only returns to each clinic the events that clinic originally submitted; clinic A cannot read clinic B's patient records; 3 integration tests verify isolation

## Phase 8: E2E Resilience Testing

- [x] Toxiproxy-based E2E test suite: `tests/docker-compose.test.yml` + `tests/real_world_bhutan_test.sh`
- [x] 2G network simulation: 600ms latency, 100ms jitter, 30 KB/s bandwidth throttle via Toxiproxy
- [x] Mid-sync connection cut: proxy disabled after 2s, client sync interrupted at the TCP layer
- [x] Reconnection and resume: VV-based deduplication ensures only missing events are retransmitted
- [x] Initiator wait-for-EOF: `sync()` now blocks until the responder closes the connection, preventing premature socket reset
- [x] End-to-end integrity check: 5000 events generated offline, transferred, cut, resumed, and verified on server with zero loss or duplication
- [x] Color-coded test output: PERFECT / GOOD / CRITICAL metrics displayed per phase of the scenario
- [x] CI-ready: GitHub Actions workflow snippet documented in `tests/README.md`

## Phase 9: PKI Multi-Nœud

- [x] `zamsync sign <clinic-dir> --ca <hub-dir>` -- signs a clinic node cert with the hub CA; multiple clinic nodes share the same CA root without each generating their own CA
- [x] `zamsync keygen` generates the hub CA + hub node cert; clinics receive a cert signed by `sign`, not their own CA
- [x] mTLS multi-clinic tests: hub CA signs Clinic A and Clinic B; rogue node with its own CA is rejected at TLS handshake (2 integration tests in `zamsync-network`)
- [x] WAL key rotation: `zamsync rekey <data-dir> --old-key <path> --new-key <path>` -- re-encrypts all WAL records with a new key atomically (tmp file + rename)
- [x] Clippy `FromStr` trait: `PayloadSchema` and `AccessPolicy` now implement `std::str::FromStr` instead of plain `from_str` methods

## Phase 10: Test Coverage

### What is well covered today

- [x] WAL durability: roundtrip, CRC corruption, crash recovery, automatic truncation
- [x] Convergence: bidirectional sync, idempotence, 2-node split-brain, deterministic merge (LogSorter)
- [x] Compaction: events dropped after peer confirmation, sync of a new peer post-compaction
- [x] Access control: `All` vs `OwnOnly`, isolation verified for clinic A / clinic B
- [x] TCP transport: end-to-end sync, batching >256 events, idempotence
- [x] TLS/mTLS: valid CA chain, rogue node rejection (different CA)
- [x] WAL encryption: encrypt/decrypt roundtrip, clear error without key (fix: no silent truncation)
- [x] E2E network tests (Toxiproxy): 2G 600ms + jitter + mid-sync cut, 5,000 events with zero loss

### Gaps Tier 1 -- Consistency Invariants (critical)

- [x] **HLC monotonicity**: multiple successive `submit()` calls → HLC strictly increasing; clock rollback absorbed by logical counter
- [x] **VersionVector operations**: `update()` (never decreases), `find_gaps()` (inclusive start seq, empty local, partial overlap, at-scale with 200 peers) all unit-tested
- [x] **Proven idempotence**: applying the same event batch 3× → exactly one copy in the WAL, VV at correct seq
- [x] **3+ node convergence**: 3 nodes in full split-brain, full mesh sync → identical 6-event sorted streams and matching VVs on all three

### Gaps Tier 2 -- Advanced Durability (important)

- [x] **WAL key rotation**: `rekey` full roundtrip (5 records), old key rejected after rekey, non-contiguous seqs preserved
- [ ] **Concurrent writes**: multiple threads calling `submit()` simultaneously → no lost events, consistent sequences
- [ ] **Compaction during active sync**: compaction runs while a peer is syncing → no corruption or loss
- [x] **Out-of-order messages**: `EventBatch` received before `Handshake` → events applied cleanly, no panic, consistent state
- [x] **WAL corruption mid-record**: magic bytes and version byte of a mid-file record corrupted → recovery stops at the correct boundary

### Gaps Tier 3 -- Edge Cases (nice to have)

- [x] **Oversized frames**: payload at MAX_FRAME_SIZE (64 MB) rejected by `write_frame`; oversized length field in `FrameBuffer` rejected before allocation
- [ ] **Expired TLS certificate**: cert with `not_after` date past → explicit rejection at handshake
- [ ] **Disk full**: `submit()` when ENOSPC → error propagated, WAL not corrupted
- [x] **Clock jump**: system clock rolls back sharply → HLC logical counter absorbs the jump, monotonicity preserved
- [x] **VersionVector with 200 peers**: `find_gaps()` correct for all entries at scale
- [ ] **CLI tests**: each command executed as a real process against a real node (`cargo test --features integration`)

## Phase 11: Database Compatibility and Ecosystem

### Projection Service

- [ ] `zamsync project <data-dir> --target postgres://...` -- official projection service; reads the ZamSync WAL and inserts events into a target database via parameterized queries (zero SQL injection)
- [ ] Persistent checkpoint: resumes from the last projected `seq` after restart; no duplicates in target database
- [ ] Configurable batch size: `--batch-size 100` to group inserts and reduce network round-trips
- [ ] Database support:
  - [ ] **PostgreSQL** -- `INSERT ... ON CONFLICT DO NOTHING` on `(origin_node, seq)`
  - [ ] **MySQL / MariaDB** -- `INSERT IGNORE INTO ...`
  - [ ] **SQLite** -- local projection for embedded devices without PG
  - [ ] **MongoDB** -- upsert on `{origin: node_id, seq: seq}`
  - [ ] **ClickHouse** -- append-only table for health/IoT event analytics
- [ ] Dry-run mode: `--dry-run` prints events that would be projected without touching the database

### Event Stream

- [ ] `zamsync stream <data-dir>` -- exposes an SSE endpoint (`text/event-stream`) or WebSocket for real-time consumers; eliminates polling `zamsync audit` in a loop
- [ ] Filter by `--node <id>` and `--since <seq>` on the stream
- [ ] Enables React/Vue frontends to consume events directly via `EventSource` without polling

### Client SDKs

- [ ] **Python SDK** (`pip install zamsync`) -- `ZamSyncClient.submit()`, `ZamSyncClient.stream()`, connects via HTTP to the embedded REST server
- [ ] **Node.js / TypeScript SDK** (`npm install zamsync-client`) -- same, for Express/NestJS backends and Next.js frontends

### CI/CD Integration

- [x] Automated release: `workflow_dispatch` with version input → bump `Cargo.toml` + commit + tag → `build-release.yml` triggered
- [x] Multi-platform binaries: x86_64-linux, aarch64-linux, armv7-linux, x86_64-windows
- [x] Multi-arch Docker image on GHCR: `latest`, `1.x`, `1.x.y`
- [x] GitHub Release with SHA-256 checksums and auto-generated release notes
- [x] Animated terminal demos (GIF): quickstart, mTLS security, WAL encryption, access control (`docs/demos/`)
- [ ] Helm chart for Kubernetes deployment (hub as Deployment, clinics as DaemonSet)
- [ ] Reusable GitHub Actions: `uses: zamsync/actions/deploy-hub@v1`

## Phase 12: REST API and Integrations

- [x] **Embedded REST API** (`zamsync serve --http 0.0.0.0:8080`) -- `POST /submit`, `GET /events?since=<seq>`, `GET /health`; integration from any language without an SDK
- [x] **SSE Event Stream** (`GET /events/stream`) -- real-time push to React/Vue frontends via `EventSource`
- [ ] **Python SDK** (`pip install zamsync`) -- `ZamSyncClient.submit()`, `ZamSyncClient.stream()`
- [ ] **Node.js / TypeScript SDK** (`npm install zamsync-client`)

## Phase 13: Field Simulation and Performance Evidence

Objective: prove with reproducible metrics that ZamSync outperforms alternatives
(IPFS, rsync) for offline-first sync on constrained hospital networks.

- [x] **Docker + Toxiproxy topology**: 1 hub + N clinics in parallel containers (`CLINIC_COUNT`), Toxiproxy per clinic, runs in CI and locally -- no VMs, no Ansible
- [x] **Network profiles via Toxiproxy**: Bhutan 2G (600ms + 30 kbps), satellite (1200ms + 100 kbps), urban 3G (80ms + 1 Mbps)
- [x] **Parallel scenario**: all clinics generate events offline then sync simultaneously; hub convergence verified
- [x] **Self-contained HTML report**: Chart.js embedded -- sync duration per clinic, bandwidth (ZamSync actual vs IPFS estimated), memory footprint, per-event wire overhead
- [x] **ZamSync vs IPFS comparison table**: mTLS, encryption at rest, access control, deterministic ordering, RAM footprint, binary size, ARM support
- [x] **GitHub Actions workflow** (`e2e-network.yml`): runs on every PR, uploads HTML report as artifact
- [ ] **Mid-sync cut test**: cut Toxiproxy proxy mid-sync, verify resume with zero data loss (already tested in `real_world_bhutan_test.sh` for single node; extend to multi-clinic)
- [ ] **Satellite profile deep run**: 8 clinics x 2000 events on satellite profile; publish report to GitHub Pages
- [ ] **Multi-run aggregation**: run 3 scenarios back-to-back, aggregate stats into a single report (mean, p95 sync time)
- [ ] **CI integration**: GitHub Actions workflow that runs the Vagrant simulation on a Linux runner and publishes the report as a GitHub Pages artifact

## Phase 14: Concurrent Hub

Discovered during Phase 13 field simulation: the hub served one peer at a time
(single-thread accept loop in `src/cmd/serve.rs`). With 4 clinics syncing in
parallel, they queued -- total wall time = sum of individual sync times instead
of max. At 30 kbps / 600ms latency, 4 clinics took 14s instead of the expected ~3-4s.

- [x] **Concurrent peer handling**: spawn one named thread per accepted connection (`sync-peer-N` / `tls-peer-N`); hub processes N clinics simultaneously; each worker opens its own `ZamEngine` instance -- no shared mutable state
- [x] **Connection limit flag**: `--max-peers 16` (default) caps concurrent sessions; stdlib counting semaphore, no external dependencies; works for both TCP and TLS modes
- [x] **Backpressure**: when at `--max-peers` capacity, the accept loop blocks after accepting -- the connected client waits for a slot instead of being rejected; OS accept queue absorbs bursts
- [x] **Correctness test**: 4-client concurrent hub test (`test_concurrent_hub_four_clients`) with a `Barrier` to synchronize all clients, verifies 20 events converge on hub with no deadlock or data loss
- [ ] **Benchmark**: re-run Phase 13 Docker simulation after fix; expected total sync time ~3-4s for 4 simultaneous clinics at 30 kbps (was 14s sequential)

## First-Deployment Target

ZamSync is a generic sync engine. The reference scenario is the Bhutan ePIS
(electronic patient information system), but the architecture is domain-agnostic
and applicable to any offline-first use case on intermittent networks.

Validated use cases:
- Field data collection (rural health, agriculture, NGOs)
- Audit log replication between sites without central cloud
- IoT sensor sync under degraded connectivity
- Multi-site event sourcing with tenant isolation (`--policy own`)

Target hardware constraints:
- Nodes: ARM64 / ARMv7 (Raspberry Pi class), 512 MB RAM minimum
- Payload: structured JSON events, typically 1-10 KB
- Acceptable sync latency: seconds to hours depending on connectivity
