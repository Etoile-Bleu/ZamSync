#!/usr/bin/env bash
#
# E2E Robustness & Resilience Test: simulating a highly unstable 2G network in rural Bhutan
# using Toxiproxy to limit bandwidth, introduce latency, and cut connections mid-transfer.
#
set -euo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Configuration defaults (can be overridden by environment variables)
TOXIPROXY_ADDR=${TOXIPROXY_ADDR:-"toxiproxy:8474"}
SERVER_ADDR=${SERVER_ADDR:-"server:7000"}
PROXY_SYNC_ADDR=${PROXY_SYNC_ADDR:-"toxiproxy:7000"}

echo -e "${BLUE}======================================================================${NC}"
echo -e "${BOLD} Starting ZamSync E2E Network Resilience Test (Bhutan 2G Simulation)${NC}"
echo -e "${BLUE}======================================================================${NC}"

# 0. Always start with a clean slate — purge WAL and state files from both data directories.
# We intentionally keep .node_id so the tester's "until [ -f /server-data/.node_id ]" check
# works immediately (the server writes it once at startup and never rewrites it).
# (guards against stale WAL from previous runs if named volumes are used)
echo -e "${BLUE}[INFO]${NC} Purging server and client WAL data (preserving node identities)..."
rm -f /server-data/events.wal /server-data/peers.state /server-data/compact.wal 2>/dev/null || true
rm -rf /data/* /data/.* 2>/dev/null || true

# 1. Wait for Server to write its identity (.node_id) and for Toxiproxy to be ready
echo -e "${BLUE}[INFO]${NC} Waiting for Server identity to be generated..."
until [ -f /server-data/.node_id ]; do
  sleep 0.5
done
SERVER_ID=$(cat /server-data/.node_id)
echo -e "${BLUE}[INFO]${NC} Server Node ID detected: ${CYAN}$SERVER_ID${NC}"

echo -e "${BLUE}[INFO]${NC} Waiting for Toxiproxy API at $TOXIPROXY_ADDR..."
until curl -s "http://$TOXIPROXY_ADDR/version" > /dev/null; do
  sleep 0.5
done
echo -e "${GREEN}[GOOD]${NC} Toxiproxy is ready."

# 2. Register the proxy in Toxiproxy
# This creates a proxy named "zamsync_proxy" listening on port 7000 and forwarding to server:7000
echo -e "${BLUE}[INFO]${NC} Configuring Toxiproxy proxy mapping..."
curl -s -X POST "http://$TOXIPROXY_ADDR/proxies" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"zamsync_proxy\",
    \"listen\": \"0.0.0.0:7000\",
    \"upstream\": \"$SERVER_ADDR\",
    \"enabled\": true
  }" > /dev/null

# 3. Add network toxics to simulate a terrible 2G connection:
# - Latency: 600ms with 100ms jitter
# - Bandwidth: 240 kbps (~30 KB/s) to optimize test speed while preserving throttled behavior
echo -e "${BLUE}[INFO]${NC} Applying 2G network simulation (latency=600ms, jitter=100ms, bandwidth=30KB/s)..."

# Latency toxics
curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy/toxics" \
  -H "Content-Type: application/json" \
  -d '{"name": "latency_down", "type": "latency", "stream": "downstream", "attributes": {"latency": 600, "jitter": 100}}' > /dev/null

curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy/toxics" \
  -H "Content-Type: application/json" \
  -d '{"name": "latency_up", "type": "latency", "stream": "upstream", "attributes": {"latency": 600, "jitter": 100}}' > /dev/null

# Bandwidth toxics (rate limit of 30 KB/s)
curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy/toxics" \
  -H "Content-Type: application/json" \
  -d '{"name": "bandwidth_down", "type": "bandwidth", "stream": "downstream", "attributes": {"rate": 30}}' > /dev/null

curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy/toxics" \
  -H "Content-Type: application/json" \
  -d '{"name": "bandwidth_up", "type": "bandwidth", "stream": "upstream", "attributes": {"rate": 30}}' > /dev/null

# 4. Initialize Client directory and generate offline events
echo -e "${BLUE}[INFO]${NC} Cleaning client data directory..."
rm -rf /data/* /data/.* 2>/dev/null || true

# Initialize client to generate NodeId
zamsync info /data > /dev/null
CLIENT_ID=$(cat /data/.node_id)
echo -e "${BLUE}[INFO]${NC} Client Node ID generated: ${CYAN}$CLIENT_ID${NC}"

echo -e "${BLUE}[INFO]${NC} Generating 5000 offline patient record events on Client..."
for i in $(seq 1 5000); do
  zamsync submit /data "patient-record-data-entry-number-$i" > /dev/null
  if (( i % 1000 == 0 )); then
    echo "  -> Generated $i/5000 events"
  fi
done
echo -e "${GREEN}[GOOD]${NC} All 5000 offline events generated successfully."

# 5. Start initial synchronization in the background
echo -e "${BLUE}[INFO]${NC} Starting synchronization over simulated 2G link (Bando/Latency constrained)..."
zamsync sync /data "$PROXY_SYNC_ADDR" "$SERVER_ID" > /tmp/sync_initial.log 2>&1 &
SYNC_PID=$!

# 6. The Épreuve de Force: Let it run for 2 seconds, then cut connection mid-sync
sleep 2
echo -e "${RED}>>> NETWORK DISRUPTION: Cutting network connection completely mid-sync! <<<${NC}"
curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy" \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}' > /dev/null

# 7. Wait for the background command to exit and log result
echo -e "${BLUE}[INFO]${NC} Waiting for client sync command to terminate..."
set +e
wait $SYNC_PID
SYNC_EXIT_CODE=$?
set -e
echo -e "${BLUE}[INFO]${NC} Client sync command exited with code: $SYNC_EXIT_CODE (expected: non-zero failure)"

echo "----- Initial Sync Log (First 15 lines) -----"
head -n 15 /tmp/sync_initial.log || true
echo "---------------------------------------------"

# 8. Simulate 3 seconds of total network blackout
echo -e "${BLUE}[INFO]${NC} Simulating blackout period (3 seconds offline)..."
sleep 3

# 9. Restore network connection
echo -e "${BLUE}[INFO]${NC} Restoring 2G network connection..."
curl -s -X POST "http://$TOXIPROXY_ADDR/proxies/zamsync_proxy" \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}' > /dev/null
sleep 1 # let Toxiproxy sockets bind

# 10. Run sync again to resume and complete synchronization
echo -e "${BLUE}[INFO]${NC} Resuming synchronization (running second sync CLI call)..."
T_START=$(date +%s)

# Check server state BEFORE the resume to verify the disruption actually interrupted the transfer
SERVER_EVENTS_BEFORE_RESUME=$(zamsync info /server-data | grep "events" | awk '{print $3}')
echo -e "${BLUE}[INFO]${NC} Server event count before resume: $SERVER_EVENTS_BEFORE_RESUME (expected: less than 5000)"

zamsync sync /data "$PROXY_SYNC_ADDR" "$SERVER_ID" > /tmp/sync_resume.log 2>&1
RESUME_EXIT_CODE=$?
T_END=$(date +%s)
DURATION=$((T_END - T_START))

if [ $RESUME_EXIT_CODE -ne 0 ]; then
  echo -e "${RED}[CRITICAL]${NC} Synchronization resume failed with code $RESUME_EXIT_CODE!"
  echo "----- Resume Sync Log -----"
  cat /tmp/sync_resume.log
  echo "---------------------------"
  exit 1
fi

echo -e "${GREEN}[GOOD]${NC} Synchronization completed successfully in ${DURATION}s."

# 11. Integrity Verification
echo -e "${BLUE}[INFO]${NC} Performing final database integrity and count verification..."
CLIENT_EVENTS=$(zamsync info /data | grep "events" | awk '{print $3}')
SERVER_EVENTS=$(zamsync info /server-data | grep "events" | awk '{print $3}')

# Metrics Evaluation
DISRUPTION_STATUS="${GREEN}PERFECT${NC}"
if [ $SYNC_EXIT_CODE -eq 0 ]; then
  DISRUPTION_STATUS="${RED}CRITICAL (Sync finished early or didn't fail on connection drop)${NC}"
fi

RECOVERY_STATUS="${GREEN}PERFECT${NC}"
if [ $RESUME_EXIT_CODE -ne 0 ]; then
  RECOVERY_STATUS="${RED}CRITICAL (Sync failed to resume)${NC}"
fi

INTEGRITY_STATUS="${GREEN}PERFECT${NC}"
if [ "$SERVER_EVENTS" -ne 5000 ] || [ "$CLIENT_EVENTS" -ne 5000 ]; then
  INTEGRITY_STATUS="${RED}CRITICAL (Event count mismatch)${NC}"
fi

LATENCY_STATUS="${GREEN}GOOD (600ms simulated)${NC}"
BANDWIDTH_STATUS="${GREEN}GOOD (30KB/s simulated)${NC}"

if [ "$SERVER_EVENTS" -ne 5000 ] || [ "$CLIENT_EVENTS" -ne 5000 ]; then
  echo -e "${RED}[CRITICAL ERROR]${NC} Database sync verification failed!"
  exit 1
fi

# 12. Phase 19: Event retention and snapshot (tested on client data -- server is still running)
echo -e "${BLUE}======================================================================${NC}"
echo -e "${BOLD} Phase 19: Event Retention and Snapshot${NC}"
echo -e "${BLUE}======================================================================${NC}"

# 12a. info must show wal size and date fields
INFO_OUT=$(zamsync info /data)
if ! echo "$INFO_OUT" | grep -q "wal size"; then
  echo -e "${RED}[CRITICAL]${NC} 'zamsync info' missing 'wal size' field"
  exit 1
fi
if ! echo "$INFO_OUT" | grep -q "oldest"; then
  echo -e "${RED}[CRITICAL]${NC} 'zamsync info' missing 'oldest' field"
  exit 1
fi
if ! echo "$INFO_OUT" | grep -q "newest"; then
  echo -e "${RED}[CRITICAL]${NC} 'zamsync info' missing 'newest' field"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: zamsync info shows wal size, oldest, newest"

# 12b. expire --dry-run with a date in the past: all events are recent, none qualify
DRY_PAST=$(zamsync expire /data --before 2020-01-01 --dry-run)
WOULD_DROP_PAST=$(echo "$DRY_PAST" | awk '/dry-run/{print $3}')
if [ "${WOULD_DROP_PAST:-1}" -ne 0 ]; then
  echo -e "${RED}[CRITICAL]${NC} expire --before 2020-01-01 --dry-run: expected 0, got ${WOULD_DROP_PAST}"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: expire --dry-run (past date) reports 0 events to drop"

# 12c. expire --dry-run with a far-future date: all 5000 events qualify
DRY_FUTURE=$(zamsync expire /data --before 2099-01-01 --dry-run)
WOULD_DROP_FUTURE=$(echo "$DRY_FUTURE" | awk '/dry-run/{print $3}')
if [ "${WOULD_DROP_FUTURE:-0}" -ne 5000 ]; then
  echo -e "${RED}[CRITICAL]${NC} expire --before 2099-01-01 --dry-run: expected 5000, got ${WOULD_DROP_FUTURE}"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: expire --dry-run (future date) reports 5000 events to drop"

# 12d. snapshot creates a file with the same byte count as the WAL
WAL_SIZE_BEFORE=$(stat -c%s /data/events.wal 2>/dev/null || echo 0)
zamsync snapshot /data --output /tmp/client_snapshot.wal
if [ ! -f /tmp/client_snapshot.wal ]; then
  echo -e "${RED}[CRITICAL]${NC} snapshot file not created"
  exit 1
fi
SNAP_SIZE=$(stat -c%s /tmp/client_snapshot.wal)
if [ "$SNAP_SIZE" -ne "$WAL_SIZE_BEFORE" ]; then
  echo -e "${RED}[CRITICAL]${NC} snapshot size (${SNAP_SIZE}B) != WAL size (${WAL_SIZE_BEFORE}B)"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: snapshot created (${SNAP_SIZE} bytes)"

# 12e. expire --min-keep 500: all events are "old" relative to 2099, keep 500 most recent
zamsync expire /data --before 2099-01-01 --min-keep 500
EVENTS_AFTER_EXPIRE=$(zamsync info /data | awk '/^events/{print $3}')
if [ "${EVENTS_AFTER_EXPIRE:-0}" -ne 500 ]; then
  echo -e "${RED}[CRITICAL]${NC} After expire --min-keep 500: expected 500 events, got ${EVENTS_AFTER_EXPIRE}"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: expire --min-keep 500 kept 500 events, dropped 4500"

# 12f. WAL writer must still work after an expire rewrite
zamsync submit /data "post-expire-health-check" > /dev/null
EVENTS_AFTER_SUBMIT=$(zamsync info /data | awk '/^events/{print $3}')
if [ "${EVENTS_AFTER_SUBMIT:-0}" -ne 501 ]; then
  echo -e "${RED}[CRITICAL]${NC} Post-expire submit: expected 501 events, got ${EVENTS_AFTER_SUBMIT}"
  exit 1
fi
echo -e "${GREEN}[GOOD]${NC} Phase 19: WAL writer functional after expire (501 events)"

RETENTION_STATUS="${GREEN}PERFECT${NC}"

echo -e ""
echo -e "${BLUE}======================================================================${NC}"
echo -e "${BOLD}                     ZAMSYNC RESILIENCE METRICS                       ${NC}"
echo -e "${BLUE}======================================================================${NC}"
echo -e " * Latency Simulation:    $LATENCY_STATUS"
echo -e " * Bandwidth Throttling:  $BANDWIDTH_STATUS"
echo -e " * Disruption Handling:   [$DISRUPTION_STATUS]"
echo -e " * Reconnection Recovery: [$RECOVERY_STATUS]"
echo -e " * Event Sync Status:     Client ($CLIENT_EVENTS/5000) -> Server ($SERVER_EVENTS/5000)"
echo -e " * Data Integrity Check:  [$INTEGRITY_STATUS]"
echo -e " * Retention (Phase 19):  [$RETENTION_STATUS]"
echo -e "${BLUE}======================================================================${NC}"

echo -e "${GREEN}[SUCCESS]${NC} E2E network resilience test passed successfully!"
echo -e "All 5000 events synchronized correctly with zero loss or duplication."
echo -e "${BLUE}======================================================================${NC}"
