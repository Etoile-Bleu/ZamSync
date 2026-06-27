# Data Management

The WAL grows indefinitely as events are appended. ZamSync provides four commands to control WAL size and lifecycle:

| Command | What it does |
|---------|-------------|
| [`expire`](#expire) | Delete events older than a date |
| [`compact`](#compact) | Remove records all peers have already received |
| [`snapshot`](#snapshot) | Copy the WAL to a backup file |
| [`rekey`](#rekey) | Re-encrypt the WAL with a new key |

These commands can be combined and scheduled as a regular maintenance job.

---

## expire

Delete events whose HLC timestamp falls before a given UTC date.

```sh
zamsync expire <data-dir> --before YYYY-MM-DD [--dry-run] [--min-keep <N>] [--key-file <path>]
```

| Flag | Description |
|------|-------------|
| `--before YYYY-MM-DD` | Required. Expire all events with an HLC timestamp before this UTC date. |
| `--dry-run` | Preview how many events would be dropped without modifying the WAL. |
| `--min-keep <N>` | Keep at least the N most recent events regardless of age (default: `0`). |
| `--key-file <path>` | Required if the WAL is encrypted. |

**Dry run first**

Always run with `--dry-run` before committing to a date:

```sh
zamsync expire /var/lib/zamsync --before 2025-01-01 --dry-run
```

```
dry-run  :  4218 events would be expired
payload  :  103 KB in expirable payloads
wal size :  892 KB
```

**Apply the expiry**

```sh
zamsync expire /var/lib/zamsync --before 2025-01-01
```

```
expire   :  dropped 4218 events, freed 103 KB
```

If there is nothing to drop:

```
expire   :  nothing to drop (all events newer than cutoff)
```

**Using `--min-keep` as a safety floor**

On resource-constrained nodes where events may arrive in bursts, use `--min-keep` to guarantee a minimum number of events are always retained regardless of age:

```sh
# Expire events older than one year, but always keep at least 1000
zamsync expire /var/lib/zamsync --before 2025-01-01 --min-keep 1000
```

**Automatic retention on startup**

`zamsync serve` and `zamsync daemon` accept `--retain <Nd>` to apply an expiry automatically on each startup:

```sh
zamsync serve /var/lib/zamsync 0.0.0.0:7000 --retain 365d
```

Output when retention fires:

```
retain   :  dropped <N> events, freed <N> KB
```

This is equivalent to running `expire --before <today minus N days>` before `serve` starts accepting connections.

---

## compact

Remove WAL records that every known peer has already received. Unlike `expire`, `compact` does not delete events by age -- it only removes records that are provably redundant because all tracked peers have acknowledged them.

```sh
zamsync compact <data-dir>
```

There are no flags. The WAL must be unencrypted (`compact` has no `--key-file` support).

**Output -- nothing to compact**

```
nothing to compact (no peers have confirmed events yet)
```

**Output -- records dropped**

```
compacted: dropped <N> WAL records
```

**When to use compact**

`compact` is most useful after a batch-sync period where many events have flowed through a hub and all clinic nodes have been confirmed up to date. Before running compact on the hub, verify all clinics have synced by checking the version vector with `zamsync info`:

```sh
zamsync info /var/lib/zamsync
```

```
node_id  :  1234567890
...
vv       :  node 1234567890 @ seq 8491
         :  peers:
    node 2748582051    8491 events
    node 3901234567    8491 events
```

All peer event counts matching the hub's total means all records are candidates for compaction.

**Compact does not reclaim disk space**

The WAL is an append-only file. Dropping records frees space logically (future compactions and appends reuse the freed region), but the file size does not shrink immediately. To reclaim physical disk space, take a [snapshot](#snapshot) and replace the WAL:

```sh
zamsync snapshot /var/lib/zamsync --output /tmp/compacted.wal
systemctl stop zamsync
cp /tmp/compacted.wal /var/lib/zamsync/events.wal
systemctl start zamsync
```

---

## snapshot

Copy the WAL to a file for backup or offline transfer. The WAL is flushed to disk before the copy, so the snapshot always reflects a consistent state.

```sh
zamsync snapshot <data-dir> --output <path> [--key-file <path>]
```

| Flag | Description |
|------|-------------|
| `--output <path>` | Required. Destination path for the snapshot file. |
| `--key-file <path>` | Required if the WAL is encrypted. |

**Output**

```
snapshot : <N> KB written to <path>
```

**Scheduled backups**

Add to cron or a systemd timer:

```sh
# /etc/cron.daily/zamsync-snapshot
#!/bin/sh
BACKUP_DIR=/var/backups/zamsync
mkdir -p "$BACKUP_DIR"
/usr/local/bin/zamsync snapshot /var/lib/zamsync \
  --output "$BACKUP_DIR/$(date +%F).wal" \
  --key-file /etc/zamsync/data.key
# Retain the last 30 snapshots
find "$BACKUP_DIR" -name '*.wal' -mtime +30 -delete
```

**Restore from snapshot**

To restore, stop the service, replace the WAL file, then restart:

```sh
systemctl stop zamsync
cp /var/backups/zamsync/2026-01-01.wal /var/lib/zamsync/events.wal
systemctl start zamsync
```

The engine replays the restored WAL on startup. Events submitted after the snapshot date will not be present.

---

## rekey

Re-encrypt the entire WAL with a new key. Use this when rotating credentials or recovering from a potential key compromise.

```sh
zamsync rekey <data-dir> --old-key <path> --new-key <path>
```

| Flag | Description |
|------|-------------|
| `--old-key <path>` | Required. Path to the current encryption key. |
| `--new-key <path>` | Required. Path to the new encryption key. |

`rekey` reads the entire WAL with `--old-key`, writes all records to a temporary file `events.wal.rekey` in the same directory, then atomically renames it over the original. The original is only replaced after the full rewrite succeeds -- a crash partway through leaves `events.wal.rekey` on disk, which can be used to continue.

**Output**

```
Re-keyed <N> WAL records in <path>
Update your --key-file to point to the new key.
```

**Key rotation workflow**

1. Generate the new key:

```sh
# Generate a standalone 32-byte key file
zamsync keygen /tmp/new-key-only
# The new key is at /tmp/new-key-only/tls/data.key
```

2. Stop the service to prevent writes during rekey:

```sh
systemctl stop zamsync
```

3. Run rekey:

```sh
zamsync rekey /var/lib/zamsync \
  --old-key /etc/zamsync/data.key \
  --new-key /tmp/new-key-only/tls/data.key
```

4. Replace the key file:

```sh
mv /tmp/new-key-only/tls/data.key /etc/zamsync/data.key
chmod 600 /etc/zamsync/data.key
```

5. Update the systemd environment file to point to the new key (if the path changed), then restart:

```sh
systemctl start zamsync
```

6. Update `--key-file` references on every node that reads this WAL (daemon instances, audit scripts, etc.).

---

## Maintenance schedule

A typical production maintenance routine for a hub:

| Frequency | Command | Purpose |
|-----------|---------|---------|
| Daily | `snapshot --output /backups/$(date +%F).wal` | Point-in-time backup |
| Weekly | `expire --before $(date -d '1 year ago' +%F) --dry-run` | Preview old event accumulation |
| Monthly | `expire --before $(date -d '1 year ago' +%F)` | Drop events older than one year |
| After full-sync confirmation | `compact` | Reclaim space from delivered records |
| On key compromise | `rekey` | Rotate encryption credentials |

For clinic nodes on SD cards or flash storage with limited write endurance, lower the retention window and add `--retain 90d` to the serve unit.
