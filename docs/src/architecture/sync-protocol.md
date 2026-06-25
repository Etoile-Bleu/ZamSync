# Sync Protocol

ZamSync's sync protocol is a bidirectional, gap-filling exchange. Two nodes compare their version vectors, compute what the other is missing, and stream only the missing events. No event is sent twice in a well-behaved session.

---

## Version vectors

A version vector (VV) is a map from node identity to the highest sequence number seen from that node:

```
{ node_id: u32 -> highest_seq: u64 }
```

Example: a hub that has received events up to sequence 50 from clinic 1 and sequence 30 from clinic 2 has the version vector `{ 1: 50, 2: 30 }`.

An absent entry is equivalent to zero. A node that has never seen any events from node 42 treats `vv.get(42)` as `0`.

### Advancing the VV

The VV only moves forward. `vv.update(node, seq)` sets the entry for `node` to `seq` if and only if `seq` is greater than the current value. This means the VV is idempotent: applying the same event twice does not change the VV.

### Finding gaps

`local_vv.find_gaps(remote_vv)` returns the list of `(node_id, start_seq)` pairs where the remote knows more than the local:

```
for each (node, remote_seq) in remote_vv:
    local_seq = local_vv.get(node)          // 0 if absent
    if remote_seq > local_seq:
        gaps.push((node, local_seq + 1))    // first missing seq is local + 1
```

If the local VV is ahead or equal for a given node, no gap is reported for that node. If the local VV does not know the node at all, the gap starts at sequence 0.

The result tells each node exactly which events to request, in the form of a starting sequence number per origin node.

---

## Message types

The protocol uses four message variants:

| Message | Direction | Content |
|---------|-----------|---------|
| `Handshake` | both ways | `node_id` + local version vector |
| `EventBatch` | both ways | `origin_node` + up to 256 events |
| `SyncComplete` | both ways | signals end of outbound stream |
| `PullRequest` | initiator only | `origin_node`, `start_seq`, `limit` (used by REST API, not the CLI sync path) |

Messages are serialized with rkyv and framed over TCP. TLS is a transparent layer below the framing.

---

## Session flow

A sync session has two roles: the **initiator** (the node that called `zamsync sync`) and the **responder** (the node running `zamsync serve`).

```
Initiator                                Responder
─────────                                ─────────
                                         (waiting for connections)
connect TCP/TLS
send Handshake{node_id, local_vv}  ──►
                                   ◄──  Handshake{node_id, local_vv}
                                   ◄──  EventBatch{...} × N
                                   ◄──  SyncComplete

apply incoming events
compute gaps from peer_vv.find_gaps(our_vv)
send EventBatch{...} × M           ──►
send SyncComplete                  ──►
                                         apply incoming events
                                         mark peer's known_vv = local_vv
wait for EOF
disconnect
```

Both sides simultaneously stream events and apply what they receive. The responder sends first; the initiator applies the responder's events before pushing its own, which means the initiator's push fills only the gaps that remain after applying the responder's events.

### Idempotency

`apply_replicated` skips any event whose sequence number is already in the local VV:

```
if event.seq <= local_vv.get(event.origin_node):
    return Ok(event.seq)    // already applied, no-op
```

This makes repeated sync sessions safe. A session interrupted mid-stream can be retried without duplicating events.

### Batch size

Events are grouped into batches of at most 256 events per `EventBatch` frame. This bounds peak memory during a session regardless of total event count. A node with 100,000 pending events sends roughly 391 frames, each processed and discarded before the next is read.

### Byte budget

The `--max-bytes` flag caps the total bytes the initiator sends in one session. The budget check happens before each outgoing `EventBatch`:

```
if bytes_sent >= max_bytes:
    stats.budget_exhausted = true
    break   // stop sending, fall through to SyncComplete
```

The session ends normally (sending `SyncComplete`). The responder's VV reflects the last batch it applied, so the next session resumes from the correct starting sequence without re-sending anything.

This is the mechanism for bandwidth-capped environments: run `zamsync sync ... --max-bytes 2M` in each available connection window and progress accumulates across sessions.

---

## Access policy

The responder applies an access policy before sending events to the initiator:

**`All` (default).** The responder sends all events it has that the initiator is missing, regardless of which node originally submitted them.

**`OwnOnly`.** The responder sends only events originally submitted by the initiator itself. If a hub node aggregates events from 10 clinics and is configured `--policy own`, each clinic receives only its own events on sync, not the other clinics' data.

The policy check is in `handle_sync_message`:

```
if policy == OwnOnly and gap.node != from_peer:
    continue    // skip this node's events
```

---

## Peer state persistence

After a `SyncComplete` is received (not sent), the engine records:

```
peers[from.0].known_vv = local_vv
```

This records that the remote peer has confirmed receiving everything in the local VV at the time the `SyncComplete` arrived. This information drives compaction: events that all peers have confirmed are safe to drop.

Peer state is saved to `peers.state` by `engine.sync()` at the end of each session.

---

## Concurrency model

`zamsync serve` handles each peer in a dedicated OS thread. A counting semaphore limits the number of concurrent threads to `--max-peers` (default 16). The main thread accepts connections as fast as possible; the semaphore blocks it before spinning up the worker thread, so accepted-but-not-yet-started connections wait in the OS accept queue.

Each worker thread opens its own `ZamEngine` instance (its own WAL reader/writer). There is no shared mutable engine state between threads. Concurrent writes from multiple peers are serialized by the OS at the `append` syscall level on platforms that guarantee atomic appends for writes under the filesystem block size.

---

## Retry behavior (initiator)

`zamsync sync` retries up to 5 times on transient errors (connection refused, I/O error, EOF during handshake). The retry delay follows exponential backoff starting at 100 ms:

| Attempt | Delay |
|---------|-------|
| 1 | no delay (first try) |
| 2 | 100 ms |
| 3 | 200 ms |
| 4 | 400 ms |
| 5 | 800 ms |

A permanent error (schema validation failure, encryption mismatch, protocol error) exits immediately without retrying.

`zamsync daemon` does not retry within a cycle; it waits for the next interval instead.
