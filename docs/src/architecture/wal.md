# Write-Ahead Log

The Write-Ahead Log (`events.wal`) is the single source of truth for a ZamSync node. Every event is appended to the WAL before being applied to application state or acknowledged to the caller. Nothing is written to an in-memory structure before first being committed to disk.

---

## Why append-only

An append-only log has three properties that are essential for an offline-first engine:

1. **Crash safety.** If the process dies after writing a record but before updating in-memory structures, the record is still on disk. On next startup, the engine replays the WAL and recovers the lost state.

2. **Deterministic replay.** Replaying the WAL in sequence always produces the same application state. This makes startup recovery and offline projection identical operations.

3. **Immutable history.** Events are never modified in place. An audit scan of the WAL always reflects exactly what was recorded and when.

---

## Binary format

Each WAL record has a 21-byte fixed header followed by a variable-length payload:

```
Offset  Size  Field
──────  ────  ─────────────────────────────────────────────────────
0       4     Magic: 0x5A 0x41 0x4D 0x21  ("ZAM!")
4       1     Version: 1 = plain, 2 = encrypted
5       4     CRC32 (big-endian) over [version, seq, len, payload]
9       8     Sequence number (big-endian u64)
17      4     Payload length in bytes (big-endian u32)
21      N     Payload (rkyv-serialized Event, or encrypted blob)
```

The magic bytes allow the scanner to detect misaligned reads. The CRC32 covers the version byte, the sequence number, the length field, and the full payload, so any single-byte corruption in any field is detected.

### Sequence numbers

Sequence numbers in the WAL are per-file monotonic counters starting at zero. They identify a record's position in the log and are used to resume a scan after a partial write. They are distinct from the `SequenceNumber` field inside an `Event`, which is the per-origin-node sequence used by version vectors.

### Payload serialization

Event payloads are serialized using [rkyv](https://rkyv.org), a zero-copy binary format. The serialized `Event` struct contains:

- `origin_node: NodeId` (u32)
- `seq: SequenceNumber` (u64)
- `hlc: Hlc` (physical: u64, logical: u32)
- `event_type: u32`
- `payload: Vec<u8>` (application data)

---

## Encryption

When a key file is provided, each WAL record's payload is encrypted with ChaCha20-Poly1305 before being written. The encrypted blob replaces the plain rkyv bytes in the payload field; the header is unchanged.

Encrypted record layout (version byte = 2):

```
Payload field = [nonce: 12 bytes][ciphertext][authentication tag: 16 bytes]
```

A fresh 12-byte random nonce is generated for every record using the OS random source. The authentication tag provides integrity: decrypting a tampered record returns an error rather than corrupted plaintext.

The CRC32 is computed over the encrypted bytes, not the plaintext. This separates the two protection layers: CRC32 detects accidental disk corruption; the authentication tag detects intentional tampering or key mismatch.

See [Security](security.md) for key generation and management details.

---

## Recovery

The WAL scanner reads forward from offset zero, validating each record's magic, version, CRC32, and payload. If a record fails validation, the scanner stops and reports the last known good position.

This handles partial writes caused by crashes. If the process was killed mid-write, the incomplete record at the tail is detected by a short read or CRC32 mismatch, and the engine recovers all records before it.

The recovery algorithm in `WalScanner::recover`:

1. Open the WAL file.
2. Scan forward, recording the sequence number and offset of each valid record.
3. On any error (short header, bad magic, bad version, bad CRC, incomplete payload), stop and log a warning.
4. Return the last valid sequence number and file offset.
5. The writer reopens the file in append mode from that offset, effectively truncating the partial tail.

One exception: if the WAL is encrypted but no key is provided, the scanner returns a configuration error immediately rather than treating encrypted records as corruption. This prevents silently truncating a valid encrypted log when the operator forgets `--key-file`.

---

## Compaction

WAL compaction removes records that all known peers have already confirmed receiving. The frontier is the minimum of each peer's confirmed version vector entry across all peers.

The compaction logic in `ZamEngine::compact`:

1. For each origin node in the local version vector, check whether every known peer has the node in its confirmed VV.
2. If all peers have confirmed it, the frontier for that node is the minimum confirmed sequence across all peers.
3. All WAL records at or below the frontier are dropped.

Compaction is conservative: if any peer has not confirmed a node's events, nothing from that node is dropped, even if other peers have confirmed it. A node with no known peers returns 0 (nothing is safe to drop).

Run `zamsync compact <data-dir>` to trigger compaction manually. Automatic compaction requires a scheduled job or integration into the `daemon` loop.

---

## Expiry (retention)

Expiry removes events whose HLC physical timestamp is older than a cutoff date, regardless of peer confirmation. It is intended for retention policies like "keep at most one year of events" on devices with limited storage.

The `--min-keep N` flag preserves the N most recent events per origin node unconditionally, preventing a misconfigured cutoff from erasing all local history.

Expiry rewrites the WAL: it scans the current file, copies every record that should be kept to a temporary file, then atomically replaces the original. The WAL is flushed before and after.

---

## Startup replay

On every `ZamEngine::new` call, the engine replays the entire WAL:

1. Scan all records in sequence order.
2. For each event, advance the HLC to `max(current_hlc, event.hlc)`.
3. Update the local version vector: `vv.update(event.origin_node, event.seq)`.
4. Call `state.apply_event(seq, &event)` to rebuild application state.

After replay, the local VV is overwritten with the WAL-derived VV. The `peers.state` file may be ahead of the WAL if a crash occurred after writing peer state but before writing WAL records; the WAL always wins.

This means startup cost is proportional to the number of events in the WAL. Use `compact` to bound this cost on long-running deployments.
