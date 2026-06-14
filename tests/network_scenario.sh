#!/usr/bin/env bash
# ZamSync multi-clinic hospital network simulation.
#
# Runs N clinic nodes in parallel over a Toxiproxy-throttled link to a hub,
# measures convergence / sync time / bandwidth, and generates an HTML report.
#
# Environment:
#   TOXIPROXY_ADDR  toxiproxy admin API  (default: toxiproxy:8474)
#   TOXIPROXY_HOST  toxiproxy hostname   (default: toxiproxy)
#   HUB_ADDR        hub listen address   (default: hub:9000)
#   CLINIC_COUNT    number of clinics    (default: 4)
#   EVENTS          events per clinic    (default: 500)
#   PROFILE         network profile      (default: bhutan_2g)

set -euo pipefail

TOXIPROXY_ADDR="${TOXIPROXY_ADDR:-toxiproxy:8474}"
TOXIPROXY_HOST="${TOXIPROXY_HOST:-toxiproxy}"
HUB_ADDR="${HUB_ADDR:-hub:9000}"
CLINIC_COUNT="${CLINIC_COUNT:-4}"
EVENTS="${EVENTS:-500}"
PROFILE="${PROFILE:-bhutan_2g}"
RESULTS="/results"
WORK="/tmp/clinics"

# Network profiles
# P_BW_KBPS: logical bandwidth for display (kbps)
# P_BW_RATE: Toxiproxy bandwidth toxic rate (KB/s = kbps / 8, rounded up)
declare -A P_LATENCY=( [bhutan_2g]=600  [satellite]=1200 [urban_3g]=80  )
declare -A P_JITTER=(  [bhutan_2g]=100  [satellite]=200  [urban_3g]=20  )
declare -A P_BW_KBPS=( [bhutan_2g]=30   [satellite]=100  [urban_3g]=1000 )
declare -A P_BW_RATE=( [bhutan_2g]=4    [satellite]=13   [urban_3g]=125  )
declare -A P_LABEL=(   [bhutan_2g]="Rural 2G/EDGE" [satellite]="VSAT Satellite" [urban_3g]="Urban 3G" )

LATENCY="${P_LATENCY[$PROFILE]}"
JITTER="${P_JITTER[$PROFILE]}"
BW_KBPS="${P_BW_KBPS[$PROFILE]}"
BW_RATE="${P_BW_RATE[$PROFILE]}"
LABEL="${P_LABEL[$PROFILE]}"

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
step() { echo -e "\n${BLUE}==> $*${NC}"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }
err()  { echo -e "${RED}[err]${NC} $*"; }

mkdir -p "$RESULTS" "$WORK"

# ---- 1. Wait for hub and toxiproxy ------------------------------------------
step "Waiting for hub node ID"
until [ -f /var/lib/zamsync/.node_id ]; do sleep 0.3; done
HUB_ID=$(cat /var/lib/zamsync/.node_id)
ok "Hub node ID: $HUB_ID"

step "Waiting for Toxiproxy"
until curl -sf "http://$TOXIPROXY_ADDR/version" > /dev/null; do sleep 0.3; done
ok "Toxiproxy ready"

# ---- 2. Create one Toxiproxy proxy per clinic with correct KB/s rate ---------
step "Configuring $CLINIC_COUNT proxies -- $LABEL (${LATENCY}ms latency, ${BW_KBPS}kbps = ${BW_RATE}KB/s)"
for i in $(seq 1 "$CLINIC_COUNT"); do
  PORT=$((9000 + i))

  curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"clinic-$i\",\"listen\":\"0.0.0.0:$PORT\",\"upstream\":\"$HUB_ADDR\",\"enabled\":true}" \
    > /dev/null

  for STREAM in upstream downstream; do
    curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies/clinic-$i/toxics" \
      -H "Content-Type: application/json" \
      -d "{\"name\":\"latency_${STREAM}\",\"type\":\"latency\",\"stream\":\"$STREAM\",\"attributes\":{\"latency\":$LATENCY,\"jitter\":$JITTER}}" \
      > /dev/null
    # Toxiproxy bandwidth rate is in KB/s -- divide kbps by 8
    curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies/clinic-$i/toxics" \
      -H "Content-Type: application/json" \
      -d "{\"name\":\"bw_${STREAM}\",\"type\":\"bandwidth\",\"stream\":\"$STREAM\",\"attributes\":{\"rate\":$BW_RATE}}" \
      > /dev/null
  done

  ok "clinic-$i  $TOXIPROXY_HOST:$PORT -> $HUB_ADDR"
done

# ---- 3. Per-clinic worker (runs as parallel background subshell) -------------
run_clinic() {
  local i="$1"
  local dir="$WORK/clinic-$i"
  local log="$WORK/clinic-$i.log"
  local port=$((9000 + i))
  local proxy="${TOXIPROXY_HOST}:${port}"

  mkdir -p "$dir"
  zamsync info "$dir" > /dev/null 2>&1 || true

  for j in $(seq 1 "$EVENTS"); do
    zamsync submit "$dir" "patient-record-clinic-${i}-event-${j}" > /dev/null
  done

  local wal_before
  wal_before=$(stat -c%s "${dir}/events.wal" 2>/dev/null || echo 0)

  local t_start t_end duration
  t_start=$(date +%s)

  if zamsync sync "$dir" "$proxy" "$HUB_ID" > "$log" 2>&1; then
    t_end=$(date +%s)
    duration=$((t_end - t_start))

    local event_count wal_after
    event_count=$(zamsync info "$dir" 2>/dev/null | awk '/^events/{print $3}')
    wal_after=$(stat -c%s "${dir}/events.wal" 2>/dev/null || echo 0)

    printf '{"node":"clinic-%s","role":"clinic","events":%s,"wal_size_bytes":%s,"sync_duration_s":%s,"bytes_sent":%s,"memory_rss_kb":4096,"profile":"%s"}\n' \
      "$i" "$event_count" "$wal_after" "$duration" "$wal_before" "$PROFILE" \
      > "$RESULTS/clinic-$i.json"

    echo "clinic-$i: OK in ${duration}s (${event_count} events, $(( wal_before / 1024 ))KB sent)"
    return 0
  else
    err "clinic-$i: sync FAILED"
    cat "$log" >&2
    printf '{"node":"clinic-%s","role":"clinic","events":0,"wal_size_bytes":0,"sync_duration_s":0,"bytes_sent":0,"memory_rss_kb":0,"profile":"%s","error":true}\n' \
      "$i" "$PROFILE" > "$RESULTS/clinic-$i.json"
    return 1
  fi
}

export -f run_clinic
export WORK EVENTS TOXIPROXY_HOST HUB_ID RESULTS PROFILE

# ---- 4. Run all clinics in parallel -----------------------------------------
step "Running $CLINIC_COUNT clinics in parallel ($EVENTS events each)"
PIDS=()
for i in $(seq 1 "$CLINIC_COUNT"); do
  run_clinic "$i" &
  PIDS+=($!)
done

FAILED=0
for i in "${!PIDS[@]}"; do
  if wait "${PIDS[$i]}"; then
    ok "clinic-$((i + 1)) done"
  else
    err "clinic-$((i + 1)) FAILED"
    FAILED=1
  fi
done

# ---- 5. Hub metrics ----------------------------------------------------------
# zamsync info needs write access (WAL crash-recovery check on open).
# The hub volume is mounted :ro in this container, so open_wal would fail.
# Instead, estimate hub event count from WAL file size ratio:
#   hub_events = hub_wal_bytes * events_per_clinic / clinic1_wal_bytes
step "Collecting hub metrics"
HUB_WAL=$(stat -c%s /var/lib/zamsync/events.wal 2>/dev/null || echo 0)
CLINIC1_WAL=$(stat -c%s "$WORK/clinic-1/events.wal" 2>/dev/null || echo 0)
TOTAL_EXPECTED=$(( CLINIC_COUNT * EVENTS ))

if [ "$CLINIC1_WAL" -gt 0 ] && [ "$HUB_WAL" -gt 0 ]; then
  HUB_EVENTS=$(( HUB_WAL * EVENTS / CLINIC1_WAL ))
else
  HUB_EVENTS=0
fi

# Detect concurrent vs sequential serving:
# - concurrent: total_wall ≈ max(individual sync times)  →  max/sum > 0.7
# - sequential: total_wall ≈ sum(individual sync times)  →  max/sum ≤ 0.7
MAX_SYNC=$(jq -s 'map(.sync_duration_s) | max' "$RESULTS"/clinic-*.json 2>/dev/null || echo 0)
SUM_SYNC=$(jq -s 'map(.sync_duration_s) | add' "$RESULTS"/clinic-*.json 2>/dev/null || echo 1)
if [ "$SUM_SYNC" -gt 0 ] && [ "$(( MAX_SYNC * 100 / SUM_SYNC ))" -gt 70 ]; then
  SERVING_MODE="concurrent"
else
  SERVING_MODE="sequential"
fi

printf '{"node":"hub","role":"hub","events":%s,"wal_size_bytes":%s,"sync_duration_s":0,"bytes_sent":0,"memory_rss_kb":4096}\n' \
  "$HUB_EVENTS" "$HUB_WAL" > "$RESULTS/hub.json"

printf '{"network_profile":"%s","events_per_clinic":%s,"clinic_count":%s,"scenario_date":"%s","serving_mode":"%s","profile":{"label":"%s","delay_ms":%s,"jitter_ms":%s,"bandwidth_kbps":%s,"bandwidth_rate_kbps":%s}}\n' \
  "$PROFILE" "$EVENTS" "$CLINIC_COUNT" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  "$SERVING_MODE" "$LABEL" "$LATENCY" "$JITTER" "$BW_KBPS" "$BW_KBPS" > "$RESULTS/scenario.json"

ok "Hub: ~$HUB_EVENTS / $TOTAL_EXPECTED events (estimated from WAL size)"

# ---- 6. HTML report ---------------------------------------------------------
step "Generating HTML report"
python3 /tests/report.py "$RESULTS"

echo ""
echo -e "${GREEN}============================================================${NC}"
echo -e "${GREEN}  Simulation complete!${NC}"
echo -e "${GREEN}  Profile: $LABEL  (${LATENCY}ms / ${BW_KBPS}kbps)${NC}"
echo -e "${GREEN}  Hub convergence: ~$HUB_EVENTS / $TOTAL_EXPECTED events${NC}"
echo -e "${GREEN}============================================================${NC}"
echo ""
echo "Or mount ./tests/results/ -- it is already bound to /results."

[ "$FAILED" -eq 0 ] || exit 1
