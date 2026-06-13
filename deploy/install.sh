#!/bin/sh
# install.sh -- install zamsync binary + systemd unit on a Linux node.
#
# Run as root after building or downloading the binary:
#   ./install.sh /path/to/zamsync
#
# After installation:
#   1. Edit /etc/zamsync/zamsync.env to set ZAMSYNC_BIND_ADDR
#   2. Run: zamsync keygen /var/lib/zamsync   (first time only)
#   3. Distribute /var/lib/zamsync/tls/ca.crt to all peer nodes
#   4. Run: systemctl enable --now zamsync

set -e

BINARY=${1:-./zamsync}
INSTALL_DIR=/usr/local/bin
SERVICE_DIR=/etc/systemd/system
CONFIG_DIR=/etc/zamsync
DATA_DIR=/var/lib/zamsync

if [ "$(id -u)" -ne 0 ]; then
    echo "error: must be run as root" >&2
    exit 1
fi

echo "installing zamsync binary..."
install -m 755 "$BINARY" "$INSTALL_DIR/zamsync"

echo "creating zamsync user and data directory..."
if ! id -u zamsync > /dev/null 2>&1; then
    useradd -r -u 1000 -s /bin/false -d "$DATA_DIR" -M zamsync
fi
install -d -m 750 -o zamsync -g zamsync "$DATA_DIR"

echo "installing systemd unit..."
install -d "$CONFIG_DIR"
if [ ! -f "$CONFIG_DIR/zamsync.env" ]; then
    install -m 640 -o root -g zamsync \
        "$(dirname "$0")/zamsync.env" "$CONFIG_DIR/zamsync.env"
    echo "  config written to $CONFIG_DIR/zamsync.env -- edit before starting"
fi
install -m 644 "$(dirname "$0")/zamsync.service" "$SERVICE_DIR/zamsync.service"
systemctl daemon-reload

echo ""
echo "installation complete."
echo ""
echo "next steps:"
echo "  1. edit $CONFIG_DIR/zamsync.env"
echo "  2. zamsync keygen $DATA_DIR   (generates TLS credentials)"
echo "  3. copy $DATA_DIR/tls/ca.crt to all peer nodes"
echo "  4. systemctl enable --now zamsync"
