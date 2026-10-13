# Concepts

ZamSync is built around three ideas that make synchronization reliable when
devices are offline or connected through a slow link: a durable event log,
timestamps that remain useful across unsynchronized clocks, and a compact way
to describe which events each node has seen.

## Write-Ahead Log (WAL)

The [Write-Ahead Log](architecture/wal.md) is ZamSync's durable notebook. Each
event is appended to `events.wal` before ZamSync applies it to application
state or acknowledges it to the caller. If a device loses power halfway
through an operation, the next startup can replay the complete records and
rebuild the state that was already recorded. The unfinished record at the end
of the file is detected and left out of recovery.

The log is append-only: old events are not edited in place. That gives the
system a stable history for audits and deterministic replay. A WAL is useful
for an offline clinic because the clinic can keep accepting events without a
network connection; synchronization can happen later from the local log.
ZamSync can also encrypt each record payload when a key file is configured,
while keeping the same recovery model.

## Hybrid Logical Clocks (HLC)

The [Hybrid Logical Clock](architecture/hlc.md) gives every event an ordering
that works across machines whose wall clocks do not agree. A plain clock can
move backward after an NTP correction, or two nodes can record events in the
same millisecond. ZamSync therefore combines the wall-clock millisecond value
(`physical`) with a logical counter (`logical`). The physical value keeps
timestamps understandable; the logical value breaks ties and absorbs clock
rollback.

When a node creates an event, `tick` advances its HLC. When it receives a
remote event, `sync` advances past both the local and remote timestamps. HLC
values compare lexicographically, so `(physical, logical)` has a deterministic
order. This provides causal ordering after nodes communicate, but events made
independently before communication still have a consistent, not necessarily
causal, order.

## Version Vectors (VV)

A [Version Vector](architecture/sync-protocol.md#version-vectors) is a small
map from each origin node to the highest sequence number seen from that node.
For example, `{ 1: 50, 2: 30 }` means the node has seen events
through those sequence numbers. A missing entry means zero, because sequence
numbers begin at zero.

Version vectors let two nodes find only the events they need. If a peer's
entry is higher than the local entry, the local node requests the missing
range instead of asking for the whole log again. `update` only moves an entry
forward, so receiving the same event twice does not change the vector.
Applying an already-known sequence is also a no-op. This makes interrupted or
repeated sync sessions safe: a connection can resume later without duplicating
events, even when a clinic has been offline for a long time.
