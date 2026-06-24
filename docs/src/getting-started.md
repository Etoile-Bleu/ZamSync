# Getting Started

Get up and running with ZamSync in under 5 minutes.

## Install

No Rust, no Cargo. One binary, zero runtime dependencies.

```bash
# Linux x86_64
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-x86_64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Linux ARM64 (Raspberry Pi 4, AWS Graviton)
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-aarch64
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Linux ARMv7 (Raspberry Pi 2 / 3)
curl -fsSL -o zamsync \
  https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-linux-armv7
chmod +x zamsync && sudo mv zamsync /usr/local/bin/

# Windows (PowerShell)
Invoke-WebRequest `
  -Uri "https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-windows-x86_64.exe" `
  -OutFile zamsync.exe
```

## Your first node

```bash
zamsync submit ./node '{"patient": "P-001", "ward": "3B", "type": "admission"}'
zamsync submit ./node '{"patient": "P-001", "type": "discharge"}'
zamsync submit ./node '{"patient": "P-002", "ward": "ICU", "type": "admission"}'

zamsync info ./node
```

```text
node_id  : 2748582051
data_dir : ./node
events   : 3
vv       : node 2748582051 @ seq 3
wal size : 1 KB
oldest   : 2026-06-17
newest   : 2026-06-17
```

## Two-node sync

```bash
# Terminal 1 -- hub node listens
zamsync serve ./hub 0.0.0.0:7000

# Terminal 2 -- clinic submits and syncs
zamsync submit ./clinic '{"patient": "P-042", "type": "visit"}'
zamsync sync   ./clinic 127.0.0.1:7000 $(cat ./hub/.node_id)
```

```text
[clinic] connecting to 127.0.0.1:7000...
[clinic] handshake ok  peer=hub
[clinic] sent 1 event
[clinic] received 0 events
[clinic] sync complete in 8ms
```

## Next steps

- [REST API](rest-api.md)
- [Error Codes](error-codes.md)
- [Deployment]()
