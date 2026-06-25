# Hybrid Logical Clocks

ZamSync assigns every event a Hybrid Logical Clock (HLC) timestamp at write time. The HLC provides a total order over all events across all nodes without requiring synchronized wall clocks or a central sequencer.

---

## The problem with wall clocks

Wall clocks are unreliable in distributed systems for two reasons:

**Clock drift.** Each machine's clock ticks at a slightly different rate. Two nodes that submit events at the "same" millisecond may have clocks that differ by hundreds of milliseconds.

**NTP corrections.** A clock that is running fast is periodically stepped backward by NTP. If ZamSync used a plain `SystemTime` timestamp, a clock correction could assign a new event a timestamp *earlier* than the previous event on the same node, breaking the ordering invariant.

A logical clock solves both problems but loses the connection to wall time, making event timestamps unreadable to humans. The HLC is a hybrid: it stays close to wall time while remaining strictly monotonic and resistant to rollbacks.

---

## Structure

The HLC is a 96-bit value composed of two fields:

```rust
pub struct Hlc {
    pub physical: u64,  // milliseconds since Unix epoch
    pub logical: u32,   // tie-breaker counter within the same millisecond
}
```

Comparison is lexicographic: `physical` is the primary key, `logical` breaks ties. This makes the HLC a total order: `(100, 0) < (100, 1) < (200, 0)`.

---

## Tick: advancing the clock on submit

When a local event is submitted, the engine calls `hlc.tick(now_ms)` where `now_ms` is the current wall-clock time in milliseconds:

```
if now_ms > self.physical:
    self.physical = now_ms
    self.logical  = 0
else:
    self.logical += 1
```

Two cases:

- **Wall clock is ahead of the HLC's physical component.** Time has advanced normally. The physical component jumps to `now_ms` and the logical counter resets to zero.
- **Wall clock is at or behind the HLC's physical component.** The wall clock has not advanced (two events in the same millisecond) or has gone backward (NTP correction). The physical component stays unchanged, and the logical counter increments to ensure the new timestamp is strictly greater than the previous one.

The logical counter therefore absorbs both high event rates (many events per millisecond) and clock rollbacks (wall clock steps backward). The HLC is always strictly monotonic on a single node.

---

## Sync: merging a remote timestamp

When an event arrives from a remote node, the engine calls `hlc.sync(now_ms, remote)` to advance the local HLC past the remote one:

```
max_phys = max(now_ms, self.physical, remote.physical)

if max_phys == self.physical and max_phys == remote.physical:
    self.logical = max(self.logical, remote.logical) + 1

else if max_phys == self.physical:
    self.logical += 1

else if max_phys == remote.physical:
    self.physical = remote.physical
    self.logical  = remote.logical + 1

else:   // wall clock is ahead of both
    self.physical = max_phys
    self.logical  = 0
```

After `sync`, the local HLC is strictly greater than both the previous local HLC and the remote HLC. This means that any event submitted *after* receiving the remote event will have a timestamp that causally follows it.

Four cases covered:

| Scenario | Result |
|----------|--------|
| Local physical is highest | Increment local logical |
| Remote physical is highest | Adopt remote physical, increment its logical |
| Wall clock is highest | Jump to wall time, reset logical to 0 |
| Local and remote physical tie | Take max logical, increment by 1 |

---

## Monotonicity guarantees

The HLC provides the following guarantees:

1. **Strictly monotonic on one node.** Every `tick` or `sync` call produces an HLC strictly greater than the previous value.

2. **Causally ordered.** If event B is submitted after receiving event A (on any node), then `hlc(B) > hlc(A)`.

3. **Bounded drift from wall time.** The physical component of the HLC is always `>= now_ms` but stays close to wall time because it advances with every forward clock tick. It only falls behind if the machine's clock is stopped.

4. **Rollback resistant.** A backward NTP correction cannot make the HLC go backward. The logical counter absorbs the gap.

---

## Total order and sort stability

ZamSync uses `(physical, logical, origin_node)` as the sort key when projecting the event log across multiple nodes. The `origin_node` field breaks the remaining tie when two events on different nodes happened to produce identical HLC values (which can occur if clocks are nearly identical and both nodes submit in the same millisecond with the same logical counter).

This three-component key is deterministic and produces the same sort order on every node that has the same event set, regardless of the order in which events were received.

---

## What the HLC does not guarantee

The HLC does not provide strict causality for events on *different* nodes that did not communicate before submitting. If clinic A writes an event at time T and clinic B writes a different event also at approximately time T, both without syncing first, their relative order is determined by their HLC values but not by physical time. The HLC order is consistent but arbitrary in this case.

Use the `hlc_logical` field in the audit output to distinguish events on the same physical millisecond. A non-zero logical counter means the node's clock did not advance between those events.
