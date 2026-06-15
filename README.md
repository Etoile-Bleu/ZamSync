<p align="center">
  <img src="docs/assets/logo-full-600.png" width="360" alt="ZamSync">
</p>

<p align="center">
  <strong>Resilient data replication for systems that cannot assume connectivity.</strong><br>
  Works over 2G, satellite, or no internet for hours &bull; No dropped events. Ever.
</p>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/Etoile-Bleu/ZamSync/ci.yml?branch=main&label=CI&style=flat-square" alt="CI status">
  <img src="https://img.shields.io/github/v/release/Etoile-Bleu/ZamSync?style=flat-square&label=release" alt="Latest release">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license">
  <img src="https://img.shields.io/badge/rust-1.75%2B-orange?style=flat-square" alt="Rust 1.75+">
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20arm64%20%7C%20armv7%20%7C%20windows-lightgrey?style=flat-square" alt="Platforms">
  <img src="https://img.shields.io/badge/docker-ghcr.io-2496ED?style=flat-square&logo=docker" alt="Docker">
  <a href="https://github.com/sponsors/Etoile-Bleu"><img src="https://img.shields.io/badge/sponsor-%E2%9D%A4-ea4aaa?style=flat-square&logo=github-sponsors" alt="Sponsor"></a>
</p>

---

<!-- Regenerate: docker run --rm -v "${PWD}:/vhs" ghcr.io/charmbracelet/vhs docs/demos/quickstart.tape -->
![ZamSync demo](docs/demos/quickstart.gif)

---

## Two nodes. Three commands.

```bash
# Node A: start listening
zamsync serve ./a 0.0.0.0:7000

# Node B: write an event and sync
zamsync submit ./b '{"patient": "P-001", "type": "visit"}'
zamsync sync   ./b 127.0.0.1:7000 $(cat ./a/.node_id)
```

```
[node-b] handshake ok  peer=a3f2c1d8
[node-b] sent 1 event
[node-b] sync complete in 12ms
```

Connection dropped mid-transfer? Run `sync` again. Only missing events are retransmitted.

No Rust, no Cargo. One binary, zero runtime dependencies.

```bash
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-x86_64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/
```

---

## Why ZamSync exists

Built for environments where connectivity is a privilege, not a guarantee.

District clinics in rural Bhutan sync patient records to a central hospital hub over 2G cellular -- 600ms latency, 30 kbps bandwidth, and frequent power outages. ZamSync was designed to work correctly through:

- **hours or days** of complete disconnection
- **mid-transfer connection cuts** -- resume exactly where it left off
- **power loss** during a sync session -- the WAL survives crashes
- **duplicate deliveries** and partial writes -- idempotent by design

Single static binary. Runs on a Raspberry Pi 2. 512 MB RAM minimum.

**Long-term vision:** enable adoption in rural public health systems such as Bhutan's clinic network, where offline-first synchronization is not an optimization -- it is a requirement.

---

## Features

| Category | Feature |
|---|---|
| **Core** | Append-only WAL with CRC32 integrity and crash recovery |
| **Replication** | Version Vector deduplication -- no duplicate events ever |
| **Ordering** | Hybrid Logical Clocks (HLC) for total deterministic ordering |
| **Security** | Mutual TLS (mTLS) with a hub-signed PKI -- rogue nodes rejected at handshake |
| **Encryption** | WAL encryption at rest (ChaCha20-Poly1305), per-record random nonce |
| **Access control** | `--policy own` -- clinic A cannot read clinic B's records |
| **Validation** | JSON schema enforcement on submit and replicated events |
| **Observability** | Prometheus metrics, structured tracing, audit trail (JSON Lines) |
| **Operations** | WAL compaction, key rotation, daemon mode, benchmarking |
| **Efficiency** | Zstd level-3 compression on all frames, chunked batches (256 events/frame) |
| **Projection** | `zamsync project` -- project WAL events to SQLite; idempotent `INSERT OR IGNORE` on `(origin_node_id, seq)` |
| **REST API** | Embedded HTTP server (`--http`) -- `POST /submit`, `GET /events`, `GET /events/stream` (SSE) |
| **Platforms** | x86_64-linux, aarch64-linux, armv7-linux, x86_64-windows (static musl binaries) |

---

## Architecture

ZamSync uses **hexagonal architecture** -- the sync core is pure logic with no I/O. Storage and transport are pluggable adapters.

```
  +----------------------------------------------------------+
  |                    zamsync (CLI binary)                   |
  +--------------------+--------------------+-----------------+
                       |                    |
          +------------+-------+  +---------+----------+  +------------------+
          |   zamsync-core     |  | zamsync-storage    |  | zamsync-network  |
          |                    |  |                    |  |                  |
          | Event, HLC         |  | ZamEngine          |  | TcpTransport     |
          | VersionVector      |  | WalEventStore      |  | TlsTcpTransport  |
          | SyncMessage        |  | FilePeerStore      |  | wire protocol    |
          | port traits        |  | WAL (CRC32)        |  | mTLS (rustls)    |
          +--------------------+  +--------------------+  +------------------+
```

### Sync protocol

```
  Clinic (initiator)              Hospital (responder)
       |                                  |
       |-- Handshake(node_id, vv) ------->|
       |                                  |  compare VVs, find gaps
       |<-- EventBatch(events...) --------|
       |<-- EventBatch(events...) --------|  chunked, 256 events/frame
       |<-- SyncComplete -----------------|
       |                                  |
       |-- EventBatch(my_events...) ----->|
       |-- SyncComplete ----------------->|
       |                                  |
```

Events are **idempotent** -- if a session is interrupted mid-transfer, the next sync only sends what's missing.

---

## Installation

### Binary (no dependencies, recommended)

Static musl binaries -- work on any Linux, no libc version requirements.

```bash
# Linux x86_64
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-x86_64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Linux ARM64 (Raspberry Pi 4, AWS Graviton)
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-aarch64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Linux ARMv7 (Raspberry Pi 2 / 3)
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-armv7
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Windows (PowerShell)
Invoke-WebRequest `
  -Uri "https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-windows-x86_64.exe" `
  -OutFile zamsync.exe
```

Verify:
```bash
sha256sum -c SHA256SUMS.txt
zamsync info ./test-node
# Node ID: a3f2c1d8e5b6...  events: 0  peers: 0
```

### Docker

```bash
docker pull ghcr.io/etoile-bleu/zamsync:latest

docker run --rm \
  -v /var/lib/zamsync:/data \
  -p 7000:7000 \
  ghcr.io/etoile-bleu/zamsync:latest \
  serve /data 0.0.0.0:7000
```

### Build from source

```bash
git clone https://github.com/Etoile-Bleu/ZamSync.git
cd ZamSync
cargo build --release
# binary at: target/release/zamsync
```

---

## Examples

### Submit events and inspect the log

```bash
zamsync submit ./node '{"patient": "P-001", "ward": "3B", "type": "admission"}'
zamsync submit ./node '{"patient": "P-001", "type": "discharge"}'
zamsync submit ./node '{"patient": "P-002", "ward": "ICU", "type": "admission"}'

zamsync info ./node
```
```
Node ID : a3f2c1d8e5b67f90
Events  : 3
Peers   : 0
WAL     : ./node/events.wal (1.2 KB)
```

```bash
zamsync audit ./node --format json | jq '{seq: .seq, type: .payload_type, size: .payload_size}'
```
```json
{"seq": 1, "type": "json", "size": 54}
{"seq": 2, "type": "json", "size": 36}
{"seq": 3, "type": "json", "size": 44}
```

---

### Two-node sync

```bash
# Terminal 1 -- hub node listens
zamsync serve ./hub 0.0.0.0:7000

# Terminal 2 -- clinic submits and syncs
zamsync submit ./clinic '{"patient": "P-042", "type": "visit"}'
zamsync sync   ./clinic 127.0.0.1:7000 $(cat ./hub/.node_id)
```
```
[clinic] connecting to 127.0.0.1:7000...
[clinic] handshake ok  peer=hub
[clinic] sent 1 event
[clinic] received 0 events
[clinic] sync complete in 8ms
```

```bash
# Hub now has the clinic's event
zamsync info ./hub
```
```
Node ID : b7e9f2a1c4d3...
Events  : 1
Peers   : 1  (clinic a3f2c1d8...)
```

---

### Encrypted WAL + audit trail

```bash
# Generate a node with a WAL encryption key
zamsync keygen ./secure-node

# All writes are encrypted at rest
zamsync submit ./secure-node '{"sensitive": "data"}' \
  --key-file ./secure-node/data.key

# Audit requires the key
zamsync audit ./secure-node --key-file ./secure-node/data.key
```
```
2026-06-14T12:00:01Z  seq=1  node=a3f2c1d8  size=22B  sha256=4a8f2c...
```

---

### Projecting to SQLite

`zamsync project` reads the local WAL and inserts every event into a SQLite database via `INSERT OR IGNORE`, making your sync data queryable with standard SQL tools. Re-running is always safe: already-present rows are skipped via a `UNIQUE(origin_node_id, seq)` constraint.

```bash
# Basic: project all events into projection.db (default path)
zamsync project ./node

# Custom output path
zamsync project ./node --target sqlite://./data/events.db

# Dry run: preview what would be projected without writing anything
zamsync project ./node --dry-run

# Tune batch size for large WALs (default: 100)
zamsync project ./node --batch-size 500
```
```
3 projected, 0 already present
```

Query the database directly once projected:

```bash
sqlite3 ./node/projection.db \
  "SELECT origin_node_id, seq, hlc_ms, payload FROM zamsync_events ORDER BY hlc_ms;"
```

```python
import sqlite3, json
conn = sqlite3.connect("./node/projection.db")
for row in conn.execute("SELECT seq, payload FROM zamsync_events ORDER BY hlc_ms"):
    print(row[0], json.loads(row[1]))
```

Schema:

| Column | Type | Description |
|--------|------|-------------|
| `origin_node_id` | INTEGER | Node that originally submitted the event |
| `seq` | INTEGER | Sequence number on the origin node |
| `hlc_ms` | INTEGER | HLC wall-clock component (Unix ms) |
| `hlc_logical` | INTEGER | HLC logical counter (tie-breaker) |
| `event_type` | INTEGER | Application-defined event type |
| `payload` | BLOB | Raw event payload (JSON or binary) |
| `projected_at` | TEXT | ISO 8601 timestamp of when this row was inserted |

---

### Daemon mode (autonomous sync)

```bash
# Clinic node syncs automatically every 5 minutes
zamsync daemon ./clinic 192.168.1.10:7000 $(cat ./hub/.node_id) \
  --tls \
  --key-file ./clinic/data.key \
  --interval 300 \
  --metrics 0.0.0.0:9090
```
```
[daemon] starting  peer=192.168.1.10:7000  interval=300s
[daemon] sync #1   sent=3  received=12  duration=1.2s
[daemon] sleeping 300s...
[daemon] sync #2   sent=0  received=7   duration=0.8s
```

---

## PKI Setup -- Multi-Clinic Deployment

```
                    +--------------+
                    |  Hub / CA    |
                    |  (hospital)  |
                    +------+-------+
               signs       |       signs
          +----------------+--------------+
          |                               |
   +------+------+              +---------+---+
   |  Clinic A   |              |  Clinic B   |
   |  node.crt   |              |  node.crt   |
   +-------------+              +-------------+
     All share the same ca.crt -- mTLS enforced
```

```bash
# 1. Initialize the hub (once)
zamsync keygen /var/lib/hub

# 2. Sign a certificate for each clinic
zamsync sign /var/lib/clinic-a --ca /var/lib/hub
zamsync sign /var/lib/clinic-b --ca /var/lib/hub

# 3. Copy clinic directories to physical devices
scp -r /var/lib/clinic-a pi@clinic-a.local:/var/lib/zamsync

# 4. Hub serves with mTLS, per-clinic isolation, and concurrent connections
zamsync serve /var/lib/hub 0.0.0.0:7000 --tls --policy own --max-peers 32

# 5. Clinics sync automatically
zamsync daemon /var/lib/zamsync 192.168.1.10:7000 $(cat /var/lib/hub/.node_id) \
  --tls --key-file /var/lib/zamsync/data.key --interval 300
```

A rogue node that presents a certificate from a different CA is **rejected at the TLS handshake** before any data is exchanged.

![mTLS security demo](docs/demos/security.gif)

---

## WAL Encryption

The WAL is encrypted at rest using **ChaCha20-Poly1305 AEAD** with a random 96-bit nonce per record. All commands accept `--key-file`:

```bash
zamsync submit  ./node "event" --key-file ./node/data.key
zamsync serve   ./node 0.0.0.0:7000 --key-file ./node/data.key
zamsync sync    ./node 192.168.1.10:7000 <peer-id> --key-file ./node/data.key
zamsync audit   ./node --key-file ./node/data.key
zamsync compact ./node --key-file ./node/data.key
```

### Key rotation

```bash
zamsync rekey ./node --old-key ./node/data.key --new-key ./node/data.key.new
mv ./node/data.key.new ./node/data.key
```

![WAL encryption demo](docs/demos/encryption.gif)

---

## Access Control

`--policy own` on `serve` enforces per-tenant isolation server-side:

```
Clinic A submits: A1, A2, A3
Clinic B submits: B1, B2

When clinic A syncs -> receives A1, A2, A3  (not B1, B2)
When clinic B syncs -> receives B1, B2       (not A1, A2, A3)
Hub sees all events.
```

![Access control demo](docs/demos/access-control.gif)

---

## REST API

ZamSync embeds an HTTP server when `--http` is passed to `serve`. This lets any language integrate without a native SDK.

```bash
# Start hub with both TCP sync port and HTTP API
zamsync serve ./hub 0.0.0.0:9000 --http 0.0.0.0:8080

# Health check
curl http://localhost:8080/health
# {"status":"ok","node_id":"a3f2c1d8","events":42}

# Submit an event via HTTP
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"event_type": 1, "payload": {"patient": "P-001", "type": "admission"}}'
# {"seq":43,"node_id":"a3f2c1d8"}

# Fetch events since seq 40
curl 'http://localhost:8080/events?since=40'
# [{"seq":41,...}, {"seq":42,...}, {"seq":43,...}]

# Server-Sent Events stream (real-time push, polls every 500ms)
curl -N http://localhost:8080/events/stream
# data: {"seq":44,"node_id":"a3f2c1d8","event_type":1,"payload":{...}}
# data: {"seq":45,...}
```

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | `GET` | Node status and event count |
| `/submit` | `POST` | Append an event (JSON body: `event_type`, `payload`) |
| `/events` | `GET` | Fetch events; `?since=<seq>` filters to newer events |
| `/events/stream` | `GET` | SSE stream -- new events pushed as they arrive |

The HTTP server runs in a dedicated OS thread and opens a fresh engine per request -- no shared mutable state, same pattern as the CLI.

---

## Concurrent Hub

The hub spawns one thread per accepted connection so all clinics sync in
parallel. A counting semaphore prevents resource exhaustion on constrained
hardware:

```bash
# Default: up to 16 simultaneous peers
zamsync serve ./hub 0.0.0.0:7000 --policy own

# RPi cluster with 64 clinics
zamsync serve ./hub 0.0.0.0:7000 --policy own --max-peers 64
```

When `--max-peers` is reached the hub accepts the TCP connection but blocks
the session start until a slot frees -- clients queue rather than being
rejected.

---

## Prometheus Metrics

```bash
zamsync serve ./hub 0.0.0.0:7000 --metrics 0.0.0.0:9090
curl http://localhost:9090/metrics
```

| Metric | Type | Description |
|---|---|---|
| `zamsync_events_submitted_total` | Counter | Events appended to local WAL |
| `zamsync_events_sent_total` | Counter | Events sent to a peer |
| `zamsync_events_received_total` | Counter | Events received from a peer |
| `zamsync_sync_duration_seconds` | Summary | Full sync session duration (quantiles: p50, p90, p99) |
| `zamsync_vv_drift` | Gauge | Version Vector gap vs peer |

---

## Hospital Network Simulation

Multi-clinic network simulation using **Docker + Toxiproxy** -- no VMs, no Ansible,
runs on any machine that has Docker and works in CI.

```bash
# 4 clinics x 500 events, Bhutan 2G profile (600ms latency, 30 kbps)
docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit

# Report is written to tests/results/report.html
start tests\results\report.html   # Windows
open  tests/results/report.html   # Linux
```

Override profile and scale:

```bash
PROFILE=satellite EVENTS=2000 CLINIC_COUNT=8 \
  docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit
```

Network profiles (applied via Toxiproxy):

| Profile | Latency | Bandwidth | Scenario |
|---------|---------|-----------|----------|
| `bhutan_2g` (default) | 600ms ± 100ms | 30 kbps | Rural clinic, 2G/EDGE |
| `satellite` | 1200ms ± 200ms | 100 kbps | Very remote, VSAT |
| `urban_3g` | 80ms ± 20ms | 1 Mbps | Urban 3G baseline |

The simulation runs **two back-to-back scenarios** and compares them:

| Scenario | `--max-peers` | Description |
|----------|--------------|-------------|
| Sequential | 1 | Clinics queue -- baseline |
| Concurrent | 16 | Clinics sync in parallel (Phase 14) |

The generated report includes:
- Hero speedup widget (e.g. **4.3x** faster on Rural 2G/EDGE)
- Sync wall time: Sequential vs Concurrent (horizontal bar chart)
- Per-clinic sync duration comparison
- Prometheus quantile distribution (p50 / p90 / p99) scraped live from each hub
- ZamSync bytes transferred vs IPFS estimated overhead
- 9-row feature comparison table (mTLS, encryption at rest, access control, ARM support ...)

---

## Embedding in a Rust Application

```toml
[dependencies]
zamsync-storage = { git = "https://github.com/Etoile-Bleu/ZamSync.git" }
zamsync-core    = { git = "https://github.com/Etoile-Bleu/ZamSync.git" }
```

```rust
use zamsync_core::{ports::StateStore, Event, SequenceNumber, ZamResult};
use zamsync_storage::ZamEngine;

struct PatientIndex {
    records: std::collections::HashMap<String, serde_json::Value>,
}

impl StateStore for PatientIndex {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        if let Ok(record) = serde_json::from_slice(&event.payload) {
            let id: String = record["id"].as_str().unwrap_or("").to_string();
            self.records.insert(id, record);
        }
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> { None }
}

fn main() -> zamsync_core::ZamResult<()> {
    let index = PatientIndex { records: Default::default() };
    let mut engine = ZamEngine::open_wal("./data", zamsync_core::NodeId(1), index)?;
    engine.submit(1, br#"{"id": "P-001", "name": "Dorji"}"#.to_vec())?;
    Ok(())
}
```

---

## Building

```bash
cargo build --release

# ARM cross-compilation
cross build --release --target aarch64-unknown-linux-musl
cross build --release --target armv7-unknown-linux-musleabihf

# Tests
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Systemd Service

```ini
# /etc/systemd/system/zamsync.service
[Unit]
Description=ZamSync Clinic Sync Daemon
After=network.target

[Service]
Type=simple
User=zamsync
ExecStart=/usr/local/bin/zamsync daemon /var/lib/zamsync \
    192.168.1.10:7000 YOUR_HUB_NODE_ID \
    --tls \
    --key-file /var/lib/zamsync/data.key \
    --interval 300
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now zamsync
journalctl -u zamsync -f
```

---

## Docker Deployment

```yaml
# docker-compose.yml -- hub node
services:
  zamsync-hub:
    image: ghcr.io/etoile-bleu/zamsync:latest
    restart: unless-stopped
    ports:
      - "7000:7000"
      - "9090:9090"
    volumes:
      - /var/lib/zamsync:/var/lib/zamsync
    command: >
      serve /var/lib/zamsync 0.0.0.0:7000
      --tls
      --policy own
      --key-file /var/lib/zamsync/data.key
      --metrics 0.0.0.0:9090
```

---

## Releasing

1. **Actions -> Release -> Run workflow**
2. Enter the version (e.g. `1.2.0`)
3. Click Run

The pipeline bumps `Cargo.toml`, tags the commit, builds 4 platform binaries, publishes a GitHub Release with SHA-256 checksums, pushes a Docker multi-arch image to GHCR, and runs a smoke test to verify the binary and image actually work.

---

## Supporting ZamSync

ZamSync is an independent open-source project. If it saves you time or helps
you build something meaningful, consider sponsoring its development.

<a href="https://github.com/sponsors/Etoile-Bleu">
  <img src="https://img.shields.io/badge/Sponsor%20on%20GitHub-%E2%9D%A4-ea4aaa?style=for-the-badge&logo=github-sponsors" alt="Sponsor on GitHub">
</a>

Sponsorships go directly toward infrastructure, hardware for ARM testing
(Raspberry Pi nodes), and time spent on maintenance and new features.

---

## License

MIT -- see [LICENSE](LICENSE).

---

## Roadmap

See [ROADMAP.md](ROADMAP.md) for completed phases and planned work.
