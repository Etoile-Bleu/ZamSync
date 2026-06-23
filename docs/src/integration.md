# Integration Guide

ZamSync exposes three integration paths. Choose based on your constraints:

| Path | When to use |
|------|-------------|
| [REST API](#rest-api) | Any language; ZamSync runs as a sidecar process |
| [Rust crates](#rust-crates) | Embed the engine directly inside a Rust application |
| [DB Projection](#db-projection) | Read-only analytics against an existing node's WAL |

---

## REST API

Start the server with `--http` to expose the HTTP interface alongside the sync listener:

```bash
zamsync serve ./data 0.0.0.0:9000 --http 0.0.0.0:8080
```

All endpoints return `application/json`. Errors always include `error` (machine-readable code) and `message` (human-readable description) -- see [Error Codes](error-codes.md).

### Submit an event

```
POST /submit
Content-Type: application/json
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `payload` | any JSON | required | Application payload |
| `event_type` | integer | `1` | Application-defined tag |

```bash
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"event_type": 1, "payload": {"patient_id": "P-001", "type": "admission"}}'
```

```json
{"seq": 43, "node_id": "a3f2c1d8"}
```

`seq` is the local sequence number assigned to this event. `node_id` is the 8-hex-digit identifier of the node that committed it.

### Poll events

```
GET /events?since=<seq>
```

Returns all events with `seq >= since` (defaults to `0`, i.e. all events). Suitable for periodic polling.

```bash
# Fetch everything
curl http://localhost:8080/events

# Incremental: only events after seq 42
curl 'http://localhost:8080/events?since=42'
```

```json
[
  {"seq": 43, "node_id": "a3f2c1d8", "event_type": 1, "payload": {"patient_id": "P-001", "type": "admission"}},
  {"seq": 44, "node_id": "a3f2c1d8", "event_type": 2, "payload": {"patient_id": "P-002", "type": "discharge"}}
]
```

### Stream events (SSE)

```
GET /events/stream?since=<seq>
```

Keeps the connection open and pushes each new event as a Server-Sent Event. The server sends a `ping` keepalive every 15 seconds. Pass `?since=<last_seen_seq>` on reconnect to avoid replaying already-processed events.

```bash
curl -N 'http://localhost:8080/events/stream?since=0'
```

```
data: {"seq":44,"node_id":"a3f2c1d8","event_type":1,"payload":{...}}
data: {"seq":45,"node_id":"a3f2c1d8","event_type":1,"payload":{...}}
: ping
```

**JavaScript (browser / Node.js)**

```js
let lastSeq = 0;

function connect() {
  const es = new EventSource(`http://localhost:8080/events/stream?since=${lastSeq}`);

  es.onmessage = (e) => {
    const event = JSON.parse(e.data);
    lastSeq = Math.max(lastSeq, event.seq);
    handleEvent(event);
  };

  es.onerror = () => {
    es.close();
    setTimeout(connect, 2000); // reconnect; lastSeq ensures no replay
  };
}

connect();
```

**Python**

```python
import json, requests

last_seq = 0

with requests.get(
    "http://localhost:8080/events/stream",
    params={"since": last_seq},
    stream=True,
) as resp:
    for line in resp.iter_lines():
        if line.startswith(b"data: "):
            event = json.loads(line[6:])
            last_seq = max(last_seq, event["seq"])
            handle_event(event)
```

### Health check

```
GET /health
```

```json
{"status": "ok", "node_id": "a3f2c1d8", "events": 44}
```

Always returns `200 OK`. Use this for liveness probes.

---

## Rust crates

Add the following to your `Cargo.toml`:

```toml
[dependencies]
zamsync-core    = "1.3"
zamsync-storage = "1.3"
```

To run sync sessions over the network, also add:

```toml
zamsync-network = "1.3"
```

### Core types

| Type | Crate | Description |
|------|-------|-------------|
| `NodeId(u32)` | `zamsync-core` | Identifies a node in the cluster |
| `SequenceNumber(u64)` | `zamsync-core` | Monotonic local sequence number |
| `Event` | `zamsync-core` | WAL record: node, seq, HLC timestamp, type, payload |
| `ZamEngine<E, P, S>` | `zamsync-storage` | The sync engine; generic over storage and state |
| `EncryptionKey` | `zamsync-storage` | 32-byte ChaCha20-Poly1305 key |
| `SyncSession` | `zamsync-storage` | Drives a single peer sync over any `Transport` |

### Open a WAL

```rust
use zamsync_storage::{ZamEngine, FilePeerStore, WalEventStore};
use zamsync_core::NodeId;

// Your application state. Must implement StateStore.
struct MyState {
    event_count: usize,
}

impl zamsync_storage::StateStore for MyState {
    fn apply_event(
        &mut self,
        _seq: zamsync_core::SequenceNumber,
        _event: &zamsync_core::Event,
    ) -> zamsync_core::ZamResult<()> {
        self.event_count += 1;
        Ok(())
    }

    fn last_applied_seq(&self) -> Option<zamsync_core::SequenceNumber> {
        None
    }
}

let node_id = NodeId(1);
let mut engine = ZamEngine::open_wal("./data", node_id, MyState { event_count: 0 })?;
```

`open_wal` creates `data/events.wal` and `data/peers.state` on first call, then reopens them on subsequent calls. All existing WAL events are replayed into your `StateStore` during `open_wal`.

### Submit an event

```rust
let payload = serde_json::to_vec(&serde_json::json!({
    "patient_id": "P-001",
    "type": "admission"
}))?;

let seq = engine.submit(1, payload)?;
engine.sync()?; // flush WAL and peer state to disk
println!("committed seq={}", seq.0);
```

`submit` assigns a Hybrid Logical Clock timestamp, appends to the WAL, and calls `StateStore::apply_event` synchronously. `sync` flushes the WAL file and persists peer state -- call it after every submit or batch of submits.

### Read events back

```rust
// Insertion order (WAL order):
for result in engine.scan_events()? {
    let event = result?;
    println!(
        "seq={} node={:08x} type={} payload={}",
        event.seq.0,
        event.origin_node.0,
        event.event_type,
        String::from_utf8_lossy(&event.payload),
    );
}

// Causal order (HLC + NodeId) -- use this for state projection:
for result in engine.sorted_scan()? {
    let event = result?;
    // events from all nodes merged in deterministic global order
}
```

Use `scan_events` for simple sequential reads. Use `sorted_scan` when building a read model from events that arrived from multiple nodes -- it merges per-node streams by HLC timestamp, which guarantees causal ordering.

### Encryption at rest

Generate a key with the CLI:

```bash
zamsync keygen > my.key
```

Load it in your application:

```rust
use zamsync_storage::EncryptionKey;

let raw: [u8; 32] = std::fs::read("my.key")?
    .try_into()
    .map_err(|_| "key must be exactly 32 bytes")?;

let key = EncryptionKey::from_bytes(raw);
let mut engine = ZamEngine::open_wal_encrypted("./data", node_id, MyState::default(), key)?;
```

The same key must be used for every subsequent `open_wal_encrypted` call on the same directory. Mixing encrypted and unencrypted opens on the same WAL file is an error.

### Schema validation

Reject submissions that are missing required fields before they reach the WAL:

```rust
use zamsync_core::PayloadSchema;

let schema = PayloadSchema::JsonRequired(vec![
    "patient_id".to_string(),
    "type".to_string(),
]);

let mut engine = ZamEngine::open_wal("./data", node_id, MyState::default())?
    .with_schema(schema);

// This will return ZamError::Validation -- "patient_id" is missing
let bad = serde_json::to_vec(&serde_json::json!({"type": "admission"}))?;
assert!(engine.submit(1, bad).is_err());
```

### Run a sync session

The engine itself is transport-agnostic. A `SyncSession` drives the protocol over any value that implements `Transport`. `zamsync-network` provides `TcpTransport` and `TlsTcpTransport`.

**Initiator (client side)**

```rust
use zamsync_network::TcpTransport;
use zamsync_storage::SyncSession;
use zamsync_core::NodeId;

let peer_addr = "192.168.1.10:9000";
let mut transport = TcpTransport::connect(peer_addr)?;
let peer_id = NodeId(2); // known peer ID

let stats = SyncSession::new(&mut engine, &mut transport).sync_with(peer_id)?;
println!("sent={} received={}", stats.events_sent, stats.events_received);
engine.sync()?;
```

**Hub (server side)**

```rust
use zamsync_network::TcpTransport;
use zamsync_storage::SyncSession;

let mut listener = TcpTransport::bind("0.0.0.0:9000")?;
loop {
    let mut peer = listener.accept_split()?;
    let peer_id = peer.peer_id();
    // Each session gets its own engine instance; no shared mutable state.
    let mut engine = ZamEngine::open_wal("./data", node_id, MyState::default())?;
    let stats = SyncSession::new(&mut engine, &mut peer).serve_one(peer_id)?;
    engine.sync()?;
}
```

For mTLS, replace `TcpTransport` / `TcpPeerTransport` with `TlsTcpTransport` / `TlsPeerTransport`. The session API is identical.

### Access policy

By default a hub forwards all events to any peer that asks (`AccessPolicy::All`). Switch to `OwnOnly` to restrict a node to receiving only the events it originally submitted:

```rust
use zamsync_core::AccessPolicy;

let engine = ZamEngine::open_wal("./data", node_id, MyState::default())?
    .with_policy(AccessPolicy::OwnOnly);
```

---

## DB Projection

Projection exports the WAL into a relational table. Run it as a one-shot ETL step or scheduled job:

```bash
# SQLite (default: <data-dir>/projection.db)
zamsync project ./data

# SQLite at a specific path
zamsync project ./data --target /var/db/analytics.db

# PostgreSQL
zamsync project ./data --target postgres://user:pass@localhost/mydb
```

Projection is **idempotent**. Re-running against the same target skips already-present events (deduplicated on `(origin_node_id, seq)`). You can run it after every sync to keep the projection fresh.

### Schema

Both targets create the same logical schema:

```sql
CREATE TABLE zamsync_events (
    id              BIGSERIAL PRIMARY KEY,         -- or INTEGER AUTOINCREMENT on SQLite
    origin_node_id  BIGINT  NOT NULL,              -- NodeId as integer
    seq             BIGINT  NOT NULL,              -- SequenceNumber
    hlc_ms          BIGINT  NOT NULL,              -- HLC physical component (ms since epoch)
    hlc_logical     BIGINT  NOT NULL,              -- HLC logical counter (tie-breaker)
    event_type      INT     NOT NULL,              -- application-defined type tag
    payload         BYTEA   NOT NULL,              -- raw event payload bytes
    projected_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(origin_node_id, seq)
);

CREATE INDEX idx_origin_seq ON zamsync_events(origin_node_id, seq);
CREATE INDEX idx_hlc         ON zamsync_events(hlc_ms, hlc_logical);
```

`payload` is stored as raw bytes. If your application submits JSON, cast it at query time:

**SQLite**

```sql
-- Most recent 10 events in causal order
SELECT
    origin_node_id,
    seq,
    datetime(hlc_ms / 1000, 'unixepoch') AS ts,
    event_type,
    payload
FROM zamsync_events
ORDER BY hlc_ms, hlc_logical, origin_node_id
LIMIT 10;

-- Events from a specific node
SELECT * FROM zamsync_events
WHERE origin_node_id = 1
ORDER BY seq;

-- Count by event type
SELECT event_type, COUNT(*) FROM zamsync_events GROUP BY event_type;
```

**PostgreSQL**

```sql
-- Decode JSON payload inline
SELECT
    origin_node_id,
    seq,
    to_timestamp(hlc_ms / 1000.0) AS ts,
    event_type,
    convert_from(payload, 'UTF8')::jsonb AS data
FROM zamsync_events
ORDER BY hlc_ms, hlc_logical, origin_node_id
LIMIT 10;

-- Filter by payload field (requires JSON payload)
SELECT *
FROM zamsync_events
WHERE convert_from(payload, 'UTF8')::jsonb->>'type' = 'admission'
ORDER BY hlc_ms;

-- Events per node per day
SELECT
    origin_node_id,
    DATE(to_timestamp(hlc_ms / 1000.0)) AS day,
    COUNT(*)
FROM zamsync_events
GROUP BY 1, 2
ORDER BY 2, 1;
```

### Dry run

Preview what would be projected without writing anything:

```bash
zamsync project ./data --dry-run
```

```
node=1 seq=0 type=1 size=52B hlc=1750000000000
node=1 seq=1 type=1 size=48B hlc=1750000000001
42 events would be projected
```

### Batch size

For large WALs, tune the transaction batch size (default: 100):

```bash
zamsync project ./data --target postgres://... --batch-size 500
```

Larger batches reduce round-trips but increase peak memory. On constrained devices (Raspberry Pi, 512 MB RAM), keep the default or lower it.

### Incremental sync pattern

Run projection after each successful sync session to keep the read model up to date:

```bash
# Sync with hub, then refresh projection
zamsync sync ./data 192.168.1.10:9000 && \
zamsync project ./data --target postgres://localhost/mydb
```

The `&&` ensures projection only runs if sync succeeded.
