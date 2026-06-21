# REST API

ZamSync embeds an HTTP server when `--http` is passed to `serve`. Any language can integrate without a native SDK.

```bash
zamsync serve ./hub 0.0.0.0:9000 --http 0.0.0.0:8080
```

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Node status and event count |
| `POST` | `/submit` | Append an event to the WAL |
| `GET` | `/events` | Fetch events (batch) |
| `GET` | `/events/stream` | Real-time SSE push stream |
| `GET` | `/ui` | Browser status dashboard |
| `GET` | `/ui/data` | Dashboard data as JSON |

---

## GET /health

Returns node identity and total event count. Always returns `200 OK`.

```bash
curl http://localhost:8080/health
```

```json
{
  "status": "ok",
  "node_id": "a3f2c1d8",
  "events": 42
}
```

---

## POST /submit

Appends an event to the local WAL. The payload is any JSON value.

```bash
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"event_type": 1, "payload": {"patient_id": "P-001", "type": "admission"}}'
```

**Request body**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `payload` | any JSON | required | Event payload |
| `event_type` | integer | `1` | Application-defined event type tag |

**Response `200 OK`**

```json
{
  "seq": 43,
  "node_id": "a3f2c1d8"
}
```

**Error responses** -- see [Error Codes](error-codes.md).

---

## GET /events

Returns a batch of events. Use `?since=<seq>` to fetch only events after a given sequence number.

```bash
# All events
curl http://localhost:8080/events

# Events after seq 40
curl 'http://localhost:8080/events?since=40'
```

**Response `200 OK`**

```json
[
  {"seq": 41, "node_id": "a3f2c1d8", "event_type": 1, "payload": {...}},
  {"seq": 42, "node_id": "a3f2c1d8", "event_type": 1, "payload": {...}}
]
```

---

## GET /events/stream

Server-Sent Events stream. The server pushes each new event as it is committed. The connection stays open; the server sends a `ping` keepalive every 15 seconds.

Pass `?since=<seq>` on reconnect to resume without replaying already-seen events.

```bash
curl -N http://localhost:8080/events/stream
```

```
data: {"seq":44,"node_id":"a3f2c1d8","event_type":1,"payload":{...}}
data: {"seq":45,"node_id":"a3f2c1d8","event_type":1,"payload":{...}}
: ping
```

**Reconnect pattern (JavaScript)**

```js
let lastSeq = 0;
function connect() {
  const es = new EventSource(`/events/stream?since=${lastSeq}`);
  es.onmessage = e => {
    const ev = JSON.parse(e.data);
    lastSeq = Math.max(lastSeq, ev.seq);
  };
}
```

---

## GET /ui

Serves the built-in browser dashboard (HTML). Secured with `Content-Security-Policy`, `X-Frame-Options`, and `X-Content-Type-Options` headers.

---

## GET /ui/data

Returns aggregate node stats consumed by the dashboard. Can also be polled directly.

```json
{
  "node_id": "a3f2c1d8",
  "events": 42,
  "wal_size_bytes": 18432,
  "uptime_seconds": 3601,
  "oldest_event": "2025-03-01",
  "newest_event": "2025-06-20"
}
```
