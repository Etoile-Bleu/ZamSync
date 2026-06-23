# CLI -- ping

`zamsync ping` probes a peer node by exchanging a single Handshake round-trip.
It is the fastest way to verify that a hub is reachable and responding with a
valid identity -- without opening the local WAL or running a full sync.

```bash
zamsync ping <data-dir> <peer-addr> [--tls] [--count N] [--timeout MS]
```

---

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--tls` | off | Use mTLS with credentials in `<data-dir>/tls/` |
| `--count N` | `3` | Number of probes to send |
| `--timeout MS` | `5000` | Per-probe timeout in milliseconds |

---

## Examples

### Plain TCP ping

```bash
zamsync ping ./clinic1 192.168.1.100:9000
```

```
PING 192.168.1.100:9000  local-node=1084291732
  seq=1  peer=3782019481  rtt=42ms
  seq=2  peer=3782019481  rtt=39ms
  seq=3  peer=3782019481  rtt=44ms
---
3/3  loss=0%
rtt  min=39ms  avg=41ms  max=44ms
```

### mTLS ping

```bash
zamsync ping ./clinic1 192.168.1.100:9000 --tls
```

```
PING 192.168.1.100:9000  local-node=1084291732  [TLS]
  seq=1  peer=3782019481  rtt=118ms  tls=ok
  seq=2  peer=3782019481  rtt=112ms  tls=ok
  seq=3  peer=3782019481  rtt=115ms  tls=ok
---
3/3  loss=0%
rtt  min=112ms  avg=115ms  max=118ms
```

The higher RTT compared to plain TCP is the TLS handshake overhead -- normal on
first connection. `tls=ok` confirms mutual authentication succeeded.

### Single probe (scripting)

```bash
zamsync ping ./clinic1 192.168.1.100:9000 --count 1 --tls
echo "exit: $?"   # 0 = reachable, 1 = unreachable
```

`ping` exits with code `1` if **all** probes fail, making it safe to use in
shell scripts and systemd `ExecStartPre` checks.

---

## Output fields

| Field | Meaning |
|-------|---------|
| `local-node` | This node's ID (from `<data-dir>/.node_id`) |
| `peer` | Remote node's ID, extracted from the Handshake response |
| `rtt` | Round-trip time including TCP connect + (TLS handshake) + Handshake exchange |
| `tls=ok` | Mutual TLS handshake completed; both certificates are valid and trusted |
| `loss` | Percentage of probes that did not receive a response within `--timeout` |

---

## What ping measures

The RTT covers the full cost of establishing a sync connection:

1. TCP three-way handshake
2. TLS mutual handshake (if `--tls`)
3. ZamSync `Handshake` message sent and acknowledged by the peer

This is intentional: it tells you the real overhead a clinic pays before any
event data is transferred.

---

## WAL and certificate requirements

`ping` reads only two things from `<data-dir>`:

- `.node_id` -- the local node's identity (created automatically if absent)
- `tls/` -- TLS credentials (only when `--tls` is passed)

The WAL is **not opened**. `ping` works even if the WAL is locked by a running
`serve` or `daemon` process.

---

## Typical field workflow

```bash
# 1. Check that the hub is reachable over mTLS before triggering a sync
zamsync ping ./clinic1 192.168.1.100:9000 --tls --count 1 && \
  zamsync sync ./clinic1 192.168.1.100:9000 3782019481 --tls

# 2. Diagnose latency before a large sync (2G link)
zamsync ping ./clinic1 192.168.1.100:9000 --tls --count 10 --timeout 10000

# 3. Systemd health check before starting the daemon
ExecStartPre=zamsync ping /var/lib/zamsync/clinic1 hub.local:9000 --tls --count 1
```
