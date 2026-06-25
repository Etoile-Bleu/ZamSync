# Metrics

ZamSync exposes a Prometheus-compatible `/metrics` endpoint when `--metrics <addr>` is passed to `serve`, `sync`, or `daemon`. The endpoint serves text format version 0.0.4 over plain HTTP.

```sh
zamsync serve /var/lib/zamsync 0.0.0.0:7000 --metrics 0.0.0.0:9090
```

```sh
curl http://localhost:9090/metrics
```

---

## Metric reference

### Events

#### `zamsync_events_submitted_total`

| | |
|-|-|
| Type | Counter |
| Labels | none |
| Source | `engine.submit()` |

Total number of events written locally via `zamsync submit` or the REST API. Does not count events received from remote peers.

---

#### `zamsync_events_expired_total`

| | |
|-|-|
| Type | Counter |
| Labels | none |
| Source | `expire_before()` |

Total number of events removed by a retention policy (`--retain`, `zamsync expire`). Resets to zero on process restart.

---

### Sync sessions

#### `zamsync_sync_events_sent_total`

| | |
|-|-|
| Type | Counter |
| Labels | `peer` (node ID as string) |
| Source | `SyncSession::sync`, `SyncSession::serve_one` |

Total events pushed to a peer during sync sessions. The `peer` label identifies the remote node. On a hub, one series per connected clinic.

---

#### `zamsync_sync_events_received_total`

| | |
|-|-|
| Type | Counter |
| Labels | `peer` (node ID as string) |
| Source | `SyncSession::sync`, `SyncSession::serve_one` |

Total events received from a peer during sync sessions.

---

#### `zamsync_bytes_sent_total`

| | |
|-|-|
| Type | Counter |
| Labels | `peer` (node ID as string) |
| Source | `SyncSession::sync`, `SyncSession::serve_one` |

Total wire bytes sent to a peer, including control frames (Handshake, SyncComplete) and event data. Useful for bandwidth accounting on metered links.

---

#### `zamsync_budget_exhausted_total`

| | |
|-|-|
| Type | Counter |
| Labels | `peer` (node ID as string) |
| Source | `SyncSession::sync` |

Number of sync sessions terminated early because the `--max-bytes` budget was reached before all gaps were filled. A non-zero rate means the byte budget is too small to fully sync in one window; increase the budget or reduce the sync interval.

---

#### `zamsync_sync_duration_seconds`

| | |
|-|-|
| Type | Histogram |
| Labels | `role` (`initiator` or `responder`) |
| Source | `SyncSession::sync`, `SyncSession::serve_one` |

Duration of each sync session from first byte sent to last byte written. Measured separately for initiator (the node that called `sync`) and responder (the node running `serve`). Use the `_bucket`, `_count`, and `_sum` suffixes to compute percentiles.

---

### Replication state

#### `zamsync_vv_drift_events`

| | |
|-|-|
| Type | Gauge |
| Labels | `peer` (node ID as string) |
| Source | `SyncSession::sync`, `SyncSession::serve_one` |

The number of events the local node has that the peer does not, computed at handshake time. A persistently high value indicates the peer is not keeping up. A value of 0 means both nodes are in sync for all locally-known event streams.

Updated at the start of every session (both initiator and responder paths).

---

### Storage

#### `zamsync_wal_size_bytes`

| | |
|-|-|
| Type | Gauge |
| Labels | none |
| Source | `SyncSession` (after each session), `expire_before()` |

Current on-disk size of `events.wal` in bytes. Updated after each sync session and after expiry runs. Use this to track WAL growth and verify that retention policies are working.

---

#### `zamsync_wal_oldest_event_timestamp_seconds`

| | |
|-|-|
| Type | Gauge |
| Labels | none |
| Source | `expire_before()` |

Unix timestamp (seconds) of the oldest event remaining in the WAL after an expiry run. Updated only when `expire_before` is called. Use this to verify that the retention policy is actually deleting old events and not just claiming to.

---

## Prometheus scrape config

```yaml
scrape_configs:
  - job_name: zamsync
    static_configs:
      - targets:
          - hub.example.com:9090
          - clinic1.example.com:9090
          - clinic2.example.com:9090
    relabel_configs:
      - source_labels: [__address__]
        regex: '([^:]+):.*'
        target_label: instance
        replacement: '$1'
```

For dynamic clinic fleets, use file-based service discovery:

```yaml
scrape_configs:
  - job_name: zamsync
    file_sd_configs:
      - files:
          - /etc/prometheus/zamsync_targets.json
        refresh_interval: 60s
```

---

## Key PromQL queries

**Event ingestion rate (hub, last 5 minutes):**

```promql
rate(zamsync_events_submitted_total[5m])
```

**Sync throughput per peer (events/sec received by hub):**

```promql
rate(zamsync_sync_events_received_total[5m])
```

**Wire bandwidth sent per peer (bytes/sec):**

```promql
rate(zamsync_bytes_sent_total[5m])
```

**Sync session P99 latency:**

```promql
histogram_quantile(0.99, rate(zamsync_sync_duration_seconds_bucket[10m]))
```

**WAL growth rate (bytes/hour):**

```promql
rate(zamsync_wal_size_bytes[1h]) * 3600
```

**Peers with high drift (more than 1000 unsynced events):**

```promql
zamsync_vv_drift_events > 1000
```

---

## Alert rules

Add to your Prometheus alert rules file:

```yaml
groups:
  - name: zamsync
    rules:

      - alert: ZamSyncPeerNotSyncing
        expr: zamsync_vv_drift_events > 500
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Peer {{ $labels.peer }} has {{ $value }} unsynced events"
          description: >
            Node {{ $labels.instance }} has had more than 500 events
            that peer {{ $labels.peer }} has not received for over 10 minutes.
            Check network connectivity and the daemon logs.

      - alert: ZamSyncWalGrowthUnbounded
        expr: rate(zamsync_wal_size_bytes[1h]) * 3600 > 104857600
        for: 30m
        labels:
          severity: warning
        annotations:
          summary: "WAL on {{ $labels.instance }} growing faster than 100 MB/h"
          description: >
            The WAL is growing at {{ $value | humanize }}B/h.
            Verify that compaction or retention policies are configured.

      - alert: ZamSyncBudgetExhausted
        expr: rate(zamsync_budget_exhausted_total[15m]) > 0
        for: 0m
        labels:
          severity: info
        annotations:
          summary: "Byte budget exhausted for peer {{ $labels.peer }}"
          description: >
            Sync sessions to peer {{ $labels.peer }} are consistently
            hitting the --max-bytes limit. Consider increasing the budget
            or reducing the sync interval to spread transfers across windows.

      - alert: ZamSyncSessionSlow
        expr: >
          histogram_quantile(0.95,
            rate(zamsync_sync_duration_seconds_bucket{role="initiator"}[10m])
          ) > 30
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Sync sessions on {{ $labels.instance }} taking over 30s (P95)"
          description: >
            The 95th percentile sync duration has exceeded 30 seconds for 5 minutes.
            This may indicate high WAL backlog, slow network, or an overloaded hub.
```

---

## Grafana dashboard

A community Grafana dashboard is available in the repository at `deploy/grafana-dashboard.json` (if present). Import it via **Dashboards > Import > Upload JSON file**.

Panels included:

| Panel | Query |
|-------|-------|
| Events submitted/sec | `rate(zamsync_events_submitted_total[1m])` |
| Sync events sent/received per peer | `rate(zamsync_sync_events_sent_total[1m])` |
| Wire bytes per peer | `rate(zamsync_bytes_sent_total[1m])` |
| VV drift per peer | `zamsync_vv_drift_events` |
| WAL size | `zamsync_wal_size_bytes` |
| Sync duration P50/P95/P99 | `histogram_quantile(...)` |
| Budget exhaustion rate | `rate(zamsync_budget_exhausted_total[5m])` |
