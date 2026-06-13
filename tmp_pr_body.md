## Summary

- Instruments `ZamEngine::submit` and `SyncSession` via the `metrics` facade crate -- zero-cost when no recorder is installed, so embedders pay nothing
- Adds 5 Prometheus metrics covering all key operational signals
- CLI: `--metrics <addr>` flag on `serve` and `sync` starts a minimal blocking HTTP `/metrics` endpoint (no async/tokio)

## Metrics exposed

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `zamsync_events_submitted_total` | counter | -- | Events written to local WAL |
| `zamsync_sync_events_sent_total` | counter | `peer` | Events pushed per sync |
| `zamsync_sync_events_received_total` | counter | `peer` | Events pulled per sync |
| `zamsync_sync_duration_seconds` | histogram | `role=initiator/responder` | End-to-end sync latency |
| `zamsync_vv_drift_events` | gauge | `peer` | Events peer is behind us |

## Test plan

- [x] `cargo test --workspace` -- 34 tests pass
- [x] `cargo clippy -- -D warnings` -- clean
- [ ] Manual: `zamsync serve /tmp/node1 0.0.0.0:7000 --metrics 0.0.0.0:9090` then `curl http://localhost:9090/metrics`

Generated with [Claude Code](https://claude.com/claude-code)
