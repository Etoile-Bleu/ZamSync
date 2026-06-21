# ZamSync

ZamSync is an **offline-first WAL sync engine** designed for low-bandwidth, intermittent-connectivity environments (2G/3G). It is purpose-built for healthcare networks where clinics record events while disconnected and reconcile them with a central hub when connectivity is available.

## Core properties

| Property | Implementation |
|---|---|
| Offline-first | Local WAL on every node; syncs when connected |
| Causal ordering | Hybrid Logical Clocks (HLC) across nodes |
| Conflict detection | Version Vectors (VV) track divergent histories |
| Encryption | ChaCha20-Poly1305 at rest + optional mTLS in transit |
| Low footprint | ~9 MB static binary, ~7 MB RSS; runs on Raspberry Pi |

## Quick start

```bash
# Start a hub node with TCP sync + HTTP API
zamsync serve ./hub 0.0.0.0:9000 --http 0.0.0.0:8080

# Submit an event
curl -X POST http://localhost:8080/submit \
  -H 'Content-Type: application/json' \
  -d '{"payload": {"patient_id": "P-001", "type": "admission"}}'
# {"seq":1,"node_id":"a3f2c1d8"}

# Watch live via SSE
curl -N http://localhost:8080/events/stream

# Browser dashboard
open http://localhost:8080/ui
```

## Documentation

- [REST API](rest-api.md) -- all endpoints with request/response examples
- [Error Codes](error-codes.md) -- structured error reference for client developers

## Source

[github.com/Etoile-Bleu/ZamSync](https://github.com/Etoile-Bleu/ZamSync)
