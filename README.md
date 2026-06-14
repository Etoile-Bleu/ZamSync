<p align="center">
  <h1 align="center">ZamSync</h1>
  <p align="center"><strong>Offline-first synchronization engine for intermittent-connectivity deployments</strong></p>
  <p align="center">
    Deterministic WAL replication &bull; mTLS PKI &bull; WAL encryption at rest &bull; ARM64 / ARMv7 native
  </p>
</p>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/Etoile-Bleu/ZamSync/ci.yml?branch=main&label=CI&style=flat-square" alt="CI status">
  <img src="https://img.shields.io/github/v/release/Etoile-Bleu/ZamSync?style=flat-square&label=release" alt="Latest release">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license">
  <img src="https://img.shields.io/badge/rust-1.75%2B-orange?style=flat-square" alt="Rust 1.75+">
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20arm64%20%7C%20armv7%20%7C%20windows-lightgrey?style=flat-square" alt="Platforms">
  <img src="https://img.shields.io/badge/docker-ghcr.io-2496ED?style=flat-square&logo=docker" alt="Docker">
</p>

---

<!-- Regenerate GIFs: docker run --rm -v "${PWD}:/vhs" ghcr.io/charmbracelet/vhs docs/demos/quickstart.tape -->
<!-- ![ZamSync demo](docs/demos/quickstart.gif) -->

## 30-second Quick Start

No Rust, no Cargo. One binary, zero dependencies.

**Linux / Raspberry Pi:**
```bash
curl -fsSL -o zamsync https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-x86_64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/
```

**Docker:**
```bash
docker pull ghcr.io/etoile-bleu/zamsync:latest
```

**First sync between two nodes:**

```bash
# Node A -- start listening
zamsync serve ./node-a 0.0.0.0:7000

# Node B (another terminal) -- write an event and sync to A
zamsync submit ./node-b '{"patient": "P-001", "type": "visit"}'
zamsync sync   ./node-b 127.0.0.1:7000 $(cat ./node-a/.node_id)
```

```
[node-b] connecting to 127.0.0.1:7000...
[node-b] handshake ok  peer=a3f2c1d8
[node-b] sent 1 event
[node-b] sync complete in 12ms
```

Node A now has the event. If the connection drops mid-transfer, just run `sync` again -- only missing events are retransmitted.

---

## What is ZamSync?

ZamSync is a systems-level synchronization engine for environments where network connectivity is unreliable, intermittent, or extremely low-bandwidth -- think 2G links, satellite connections, or Raspberry Pi nodes in remote clinics.

It provides **deterministic, bidirectional event replication** between nodes, designed to work correctly through:

- hours or days of complete disconnection
- mid-transfer connection cuts
- duplicate deliveries and partial writes
- power loss during a sync session

The reference deployment is the **Bhutan ePIS** (electronic patient information system), where district clinics sync patient records to a central hospital hub over 2G cellular with 600ms latency and frequent blackouts.

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

# 4. Hub serves with mTLS and per-clinic isolation
zamsync serve /var/lib/hub 0.0.0.0:7000 --tls --policy own

# 5. Clinics sync automatically
zamsync daemon /var/lib/zamsync 192.168.1.10:7000 $(cat /var/lib/hub/.node_id) \
  --tls --key-file /var/lib/zamsync/data.key --interval 300
```

A rogue node that presents a certificate from a different CA is **rejected at the TLS handshake** before any data is exchanged.

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
| `zamsync_sync_duration_seconds` | Histogram | Full sync session duration |
| `zamsync_vv_drift` | Gauge | Version Vector gap vs peer |

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

## Network Resilience Test

ZamSync ships with a Toxiproxy-based E2E test simulating real Bhutan 2G conditions:

- 600ms latency + 100ms jitter + 30 KB/s bandwidth cap
- Mid-sync connection cut after 2 seconds
- 5,000 events verified with zero loss or duplication after reconnect

```bash
docker compose -f tests/docker-compose.test.yml up --build --abort-on-container-exit
```

See [tests/README.md](tests/README.md) for details.

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

## Releasing

1. **Actions -> Release -> Run workflow**
2. Enter the version (e.g. `1.2.0`)
3. Click Run

The pipeline bumps `Cargo.toml`, tags the commit, builds 4 platform binaries, publishes a GitHub Release with SHA-256 checksums, pushes a Docker multi-arch image to GHCR, and runs a smoke test to verify the binary and image actually work.

---

## License

MIT -- see [LICENSE](LICENSE).

---

## Roadmap

See [ROADMAP.md](ROADMAP.md) for completed phases and planned work.
