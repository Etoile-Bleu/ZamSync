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

## Phase 10: Compatibilité Bases de Données et Écosystème

### Projection Service (remplacement sécurisé du script shell)

- [ ] `zamsync project <data-dir> --target postgres://...` — service de projection officiel; lit le WAL ZamSync et insère les events dans une base cible via requêtes paramétrées (zéro injection SQL)
- [ ] Checkpoint persistant : reprend depuis le dernier `seq` projeté après redémarrage; aucun doublon en base cible
- [ ] Batch configurable : `--batch-size 100` pour regrouper les inserts et réduire les round-trips réseau
- [ ] Support bases de données :
  - [ ] **PostgreSQL** — `INSERT ... ON CONFLICT DO NOTHING` sur `(origin_node, seq)`
  - [ ] **MySQL / MariaDB** — `INSERT IGNORE INTO ...`
  - [ ] **SQLite** — projection locale pour appareils embarqués sans PG
  - [ ] **MongoDB** — upsert sur `{origin: node_id, seq: seq}`
  - [ ] **ClickHouse** — append-only table pour analytics sur events de santé / IoT
- [ ] Mode dry-run : `--dry-run` affiche les events qui seraient projetés sans toucher la base

### Event Stream (push au lieu de polling)

- [ ] `zamsync stream <data-dir>` — expose un endpoint SSE (`text/event-stream`) ou WebSocket que les services consommateurs écoutent en temps réel; élimine le besoin de poller `zamsync audit` en boucle
- [ ] Filtre par `--node <id>` et `--since <seq>` sur le stream pour ne recevoir que les events pertinents
- [ ] Permet aux frontends React/Vue de consommer les events directement via EventSource sans polling

### SDKs Clients

- [ ] **Python SDK** (`pip install zamsync`) — `ZamSyncClient.submit()`, `ZamSyncClient.stream()`, connexion au daemon local via socket Unix ou HTTP; remplace les appels shell
- [ ] **Node.js / TypeScript SDK** (`npm install zamsync-client`) — idem, pour backends Express/NestJS et frontends Next.js
- [ ] **REST API** embarquée (`zamsync serve --http 0.0.0.0:8080`) — `POST /submit`, `GET /events?since=<seq>`, `GET /health`, `GET /metrics`; permet l'intégration sans SDK depuis n'importe quel langage

### Intégration CI/CD

- [x] Release automatisée : `workflow_dispatch` avec saisie de version → bump `Cargo.toml` + commit + tag → `build-release.yml` déclenché
- [x] Binaires multi-plateformes : x86_64-linux, aarch64-linux, armv7-linux, x86_64-windows -- compilés nativement ou via `cross`
- [x] Image Docker multi-arch publiée sur GHCR : `latest`, `1.x`, `1.x.y` -- sans QEMU pour la compilation (binaires pré-construits via `Dockerfile.release`)
- [x] GitHub Release avec checksums SHA-256 et notes de version automatiques
- [x] Démos terminales animées (GIF) : quickstart, sécurité mTLS, chiffrement WAL, contrôle d'accès (`docs/demos/`)
- [ ] Helm chart pour déploiement Kubernetes (hub en Deployment, nœuds en DaemonSet)
- [ ] GitHub Actions réutilisable : `uses: zamsync/actions/deploy-hub@v1`

## First-Deployment Target

ZamSync est un moteur de synchronisation générique. Le scénario de référence est le Bhutan ePIS (electronic patient information system), mais l'architecture est agnostique au domaine métier et applicable à tout cas offline-first sur réseau intermittent.

Cas d'usage validés :
- Collecte de données terrain (santé rurale, agriculture, ONG)
- Réplication de logs d'audit entre sites sans cloud central
- Sync de capteurs IoT en connectivité dégradée
- Event sourcing multi-site avec isolation par tenant (`--policy own`)

Contraintes matérielles cibles :
- Nodes : ARM64 / ARMv7 (Raspberry Pi class), 512 MB RAM minimum
- Payload : événements JSON structurés, typiquement 1–10 KB
- Latence de sync acceptable : de quelques secondes à plusieurs heures selon connectivité
