# Deployment

This page covers production deployment of ZamSync on Linux. It assumes a hub-and-clinic topology: one hub node running `zamsync serve` and one or more clinic nodes running `zamsync daemon` to sync outbound.

For a local two-node lab setup, see the [Getting Started](../getting-started.md) guide.

---

## Installation

### From a pre-built release

Download the binary for your platform from the [Releases](https://github.com/Etoile-Bleu/ZamSync/releases) page and install it:

```sh
curl -sSL https://github.com/Etoile-Bleu/ZamSync/releases/latest/download/zamsync-x86_64-unknown-linux-musl.tar.gz \
  | tar -xz
install -m 755 zamsync /usr/local/bin/zamsync
```

Static musl builds are available for `x86_64`, `aarch64` (ARM64), and `armv7`. No runtime dependencies.

### From source

```sh
git clone https://github.com/Etoile-Bleu/ZamSync.git
cd ZamSync
cargo build --release
install -m 755 target/release/zamsync /usr/local/bin/zamsync
```

### Using the install script

The `deploy/install.sh` script handles binary installation, user creation, and systemd unit registration in one step:

```sh
sudo ./deploy/install.sh /path/to/zamsync
```

It creates:
- A `zamsync` system user (uid 1000, no shell, no home directory)
- `/var/lib/zamsync` owned by that user (mode 750)
- `/etc/zamsync/zamsync.env` for site-specific configuration
- The systemd unit at `/etc/systemd/system/zamsync.service`

---

## systemd: hub node

The hub runs `zamsync serve` and accepts inbound sync connections from clinics.

### Environment file

`/etc/zamsync/zamsync.env` holds all site-specific values. The systemd unit reads this file with `EnvironmentFile=`:

```ini
# /etc/zamsync/zamsync.env

ZAMSYNC_DATA_DIR=/var/lib/zamsync
ZAMSYNC_BIND_ADDR=0.0.0.0:7000
```

Optional variables for mTLS and encryption:

```ini
# Add these after running zamsync keygen
ZAMSYNC_TLS=--tls
ZAMSYNC_KEY_FILE=--key-file /etc/zamsync/data.key
```

Then reference them in the unit's `ExecStart`:

```ini
ExecStart=/usr/local/bin/zamsync serve ${ZAMSYNC_DATA_DIR} ${ZAMSYNC_BIND_ADDR} \
    ${ZAMSYNC_TLS} ${ZAMSYNC_KEY_FILE}
```

### Unit file

`deploy/zamsync.service` (already installed by `install.sh`):

```ini
[Unit]
Description=ZamSync offline-first sync engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=zamsync
Group=zamsync
EnvironmentFile=/etc/zamsync/zamsync.env
ExecStart=/usr/local/bin/zamsync serve ${ZAMSYNC_DATA_DIR} ${ZAMSYNC_BIND_ADDR}
Restart=on-failure
RestartSec=5s
TimeoutStopSec=30s
WorkingDirectory=/var/lib/zamsync
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ReadWritePaths=/var/lib/zamsync
StandardOutput=journal
StandardError=journal
SyslogIdentifier=zamsync

[Install]
WantedBy=multi-user.target
```

### Start and enable

```sh
systemctl daemon-reload
systemctl enable --now zamsync
systemctl status zamsync
```

Logs:

```sh
journalctl -u zamsync -f
```

---

## systemd: clinic node (outbound daemon)

The clinic runs `zamsync daemon` to sync periodically with the hub.

### Environment file additions

```ini
# append to /etc/zamsync/zamsync.env on the clinic

ZAMSYNC_HUB_ADDR=hub.example.com:7000
ZAMSYNC_HUB_ID=1
ZAMSYNC_INTERVAL=60
```

`ZAMSYNC_HUB_ID` is the hub's node identity, readable from the hub with `zamsync info /var/lib/zamsync`.

### Unit file

`deploy/zamsync-daemon.service`:

```ini
[Unit]
Description=ZamSync daemon - periodic outbound sync to hub
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=zamsync
Group=zamsync
EnvironmentFile=/etc/zamsync/zamsync.env
ExecStart=/usr/local/bin/zamsync daemon \
    ${ZAMSYNC_DATA_DIR} \
    ${ZAMSYNC_HUB_ADDR} \
    ${ZAMSYNC_HUB_ID} \
    --interval ${ZAMSYNC_INTERVAL:-60}
Restart=on-failure
RestartSec=10s
TimeoutStopSec=30s
WorkingDirectory=/var/lib/zamsync
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ReadWritePaths=/var/lib/zamsync
StandardOutput=journal
StandardError=journal
SyslogIdentifier=zamsync-daemon

[Install]
WantedBy=multi-user.target
```

Install and start:

```sh
install -m 644 deploy/zamsync-daemon.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now zamsync-daemon
```

---

## Docker

### Pre-built image

```sh
docker pull ghcr.io/etoile-bleu/zamsync:latest
```

### Running a hub

```sh
docker run -d \
  --name zamsync-hub \
  -p 7000:7000 \
  -v zamsync_data:/var/lib/zamsync \
  ghcr.io/etoile-bleu/zamsync:latest \
  serve /var/lib/zamsync 0.0.0.0:7000
```

With mTLS and WAL encryption (after credentials are provisioned into the volume):

```sh
docker run -d \
  --name zamsync-hub \
  -p 7000:7000 \
  -v zamsync_data:/var/lib/zamsync \
  -v /etc/zamsync/data.key:/run/secrets/data.key:ro \
  ghcr.io/etoile-bleu/zamsync:latest \
  serve /var/lib/zamsync 0.0.0.0:7000 --tls --key-file /run/secrets/data.key
```

### Docker Compose: two-node cluster

The `docker-compose.yml` in the repository root starts two nodes for local testing:

```sh
docker compose up -d

# Get node-a's ID
NODE_A_ID=$(docker compose exec node-a cat /data/.node_id)

# Trigger a sync from node-b to node-a
docker compose exec node-b zamsync sync /data node-a:7000 $NODE_A_ID

# Inspect both nodes
docker compose exec node-a zamsync info /data
docker compose exec node-b zamsync info /data
```

### Building the image

The Dockerfile uses a two-stage build. The final image is based on `debian:bookworm-slim` and contains only the binary and CA certificates (~20 MB).

```sh
docker build -t zamsync:local .

# Cross-compile for ARM (Raspberry Pi)
docker buildx build --platform linux/arm64  -t zamsync:arm64  .
docker buildx build --platform linux/arm/v7 -t zamsync:armv7  .
```

---

## PKI setup (first-time, hub)

Run once after installation before starting any services:

```sh
# 1. Generate hub credentials
sudo -u zamsync zamsync keygen /var/lib/zamsync

# 2. Move the WAL key outside the data directory
sudo mv /var/lib/zamsync/tls/data.key /etc/zamsync/data.key
sudo chmod 600 /etc/zamsync/data.key
sudo chown zamsync:zamsync /etc/zamsync/data.key

# 3. Distribute the CA certificate to all clinic nodes
cat /var/lib/zamsync/tls/ca.crt
```

For each new clinic node:

```sh
# On the hub: sign a certificate for the new clinic
sudo -u zamsync zamsync sign /tmp/clinic1 --ca /var/lib/zamsync

# Copy /tmp/clinic1/tls/ to the clinic device
scp -r /tmp/clinic1/tls/ clinic1:/var/lib/zamsync/tls/
```

See the [Security](../architecture/security.md) page for the full PKI model.

---

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| `7000` | TCP | Sync protocol (plain or mTLS). Configurable with `ZAMSYNC_BIND_ADDR`. |
| `9090` | TCP | Prometheus `/metrics` endpoint. Optional, enabled with `--metrics`. |
| `8080` | TCP | REST API. Optional, enabled with `--http`. |

Only port 7000 is required. Open it on the hub's firewall to the IP ranges of clinic nodes:

```sh
# ufw
ufw allow from 10.0.0.0/8 to any port 7000

# firewalld
firewall-cmd --permanent --add-rich-rule='rule family=ipv4 source address=10.0.0.0/8 port port=7000 protocol=tcp accept'
firewall-cmd --reload
```

---

## Production checklist

### File system

- [ ] Data directory is `chmod 750`, owned by the `zamsync` user
- [ ] `data.key` is outside the data directory (`/etc/zamsync/data.key`), `chmod 600`
- [ ] `ca.key` is stored securely and is not on clinic nodes
- [ ] WAL is on a filesystem with journaling enabled (ext4, xfs)
- [ ] Snapshot backups are scheduled (`zamsync snapshot --output /backups/$(date +%F).wal`)

### Network

- [ ] Port 7000 is open only to known peer IP ranges
- [ ] mTLS is enabled on all nodes (`--tls`)
- [ ] The REST API port (8080) is not exposed to the internet if enabled

### Service

- [ ] `systemctl enable zamsync` so the service restarts after reboot
- [ ] `Restart=on-failure` is set (included in the provided unit files)
- [ ] Log rotation is configured for journald (`/etc/systemd/journald.conf`: `SystemMaxUse=500M`)
- [ ] Retention policy is set for SD-card or flash storage nodes (`--retain 365d` on the serve unit)

### Monitoring

- [ ] Prometheus scraping is configured for `--metrics 0.0.0.0:9090`
- [ ] An alert fires when `zamsync_vv_drift_events` stays above a threshold for more than one sync interval
- [ ] An alert fires when `zamsync_wal_size_bytes` grows faster than expected

See [Metrics](metrics.md) for the full list of available metrics and example alert rules.

---

## Upgrading

ZamSync's WAL format is versioned. Minor version upgrades are always backward-compatible. Before upgrading:

1. Take a snapshot: `zamsync snapshot /var/lib/zamsync --output /backups/pre-upgrade.wal`
2. Stop the service: `systemctl stop zamsync`
3. Replace the binary
4. Start the service: `systemctl start zamsync`

The engine replays the WAL on startup. No migration step is required for minor versions.
