# Concepts

ZamSync uses a few ideas from distributed systems to keep events safe and make
them easy to exchange between nodes. This page introduces them without
requiring a background in distributed systems.

## Write-Ahead Log (WAL)

A Write-Ahead Log is like a notebook where each new entry is written at the
end. ZamSync records an event in its WAL before applying it to the node's
current state or acknowledging it. The WAL is the durable history from which
the node can rebuild its state after a restart. If a process or power loss
interrupts a write, the next startup checks the records and stops at an
incomplete or damaged tail instead of treating it as valid data. Keeping the
history append-only also makes recovery a replay of events in order. ZamSync
uses this design so a disconnected or restarted clinic can resume from the
events it has recorded. See the [WAL architecture guide](architecture/wal.md)
for recovery and record details.

## Hybrid Logical Clocks (HLC)

Two computers' wall clocks can disagree: one may run fast, or a clock may be
corrected backward. Sorting events by those clocks alone can therefore put a
new event before an older one. A Hybrid Logical Clock combines a wall-clock
time with a small counter. When time moves forward, the physical part advances
and the counter resets; when time has not advanced or has moved backward, the
counter increases. When a node receives a remote event, it advances its clock
past that event. ZamSync can thus keep events ordered across nodes without
requiring perfectly synchronized clocks or a central timekeeper. Events
created independently may still have the same HLC, so the origin node is used
as a deterministic tie-breaker. Read the [HLC guide](architecture/hlc.md) for
the update rules and their limits.

## Version Vectors (VV)

A Version Vector is a small progress list: for each node, it records the
highest sequence number seen from that node. For example, `{clinic A: 12,
clinic B: 7}` means the receiver has seen events through sequence 12 from A
and through 7 from B. When two ZamSync nodes compare their vectors, each can
see which origin has advanced further and request events starting after its
own recorded sequence. This helps a sync send missing events without sending
the entire history again. The vector also advances idempotently, so applying
the same sequence twice does not move its progress backward or count it as
new progress; the engine checks event origin and sequence to reject duplicates.
See the [sync protocol guide](architecture/sync-protocol.md) for gap detection
and the exchange flow.
