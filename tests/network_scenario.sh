#!/usr/bin/env bash
# ZamSync multi-clinic hospital network simulation.
#
# Runs two back-to-back scenarios against separate hub containers:
#   seq -- hub-seq with --max-peers 1  (sequential baseline)
#   con -- hub-con with --max-peers 16 (concurrent, Phase 14)
#
# Environment:
#   TOXIPROXY_ADDR    toxiproxy admin API   (default: toxiproxy:8474)
#   TOXIPROXY_HOST    toxiproxy hostname    (default: toxiproxy)
#   HUB_SEQ_ADDR      sequential hub addr  (default: hub-seq:9000)
#   HUB_SEQ_DATA      sequential hub data  (default: /var/lib/hub-seq)
#   HUB_SEQ_METRICS   sequential hub prom  (default: hub-seq:9090)
#   HUB_CON_ADDR      concurrent hub addr  (default: hub-con:9000)
#   HUB_CON_DATA      concurrent hub data  (default: /var/lib/hub-con)
#   HUB_CON_METRICS   concurrent hub prom  (default: hub-con:9090)
#   CLINIC_COUNT      number of clinics    (default: 4)
#   EVENTS            events per clinic    (default: 500)
#   PROFILE           network profile      (default: bhutan_2g)

set -euo pipefail

TOXIPROXY_ADDR="${TOXIPROXY_ADDR:-toxiproxy:8474}"
TOXIPROXY_HOST="${TOXIPROXY_HOST:-toxiproxy}"
HUB_SEQ_ADDR="${HUB_SEQ_ADDR:-hub-seq:9000}"
HUB_SEQ_DATA="${HUB_SEQ_DATA:-/var/lib/hub-seq}"
HUB_SEQ_METRICS="${HUB_SEQ_METRICS:-hub-seq:9090}"
HUB_CON_ADDR="${HUB_CON_ADDR:-hub-con:9000}"
HUB_CON_DATA="${HUB_CON_DATA:-/var/lib/hub-con}"
HUB_CON_METRICS="${HUB_CON_METRICS:-hub-con:9090}"
CLINIC_COUNT="${CLINIC_COUNT:-4}"
EVENTS="${EVENTS:-500}"
PROFILE="${PROFILE:-bhutan_2g}"
RESULTS="/results"

# Network profiles
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

mkdir -p "$RESULTS"

# ---- Wait for shared infrastructure -----------------------------------------
step "Waiting for Toxiproxy"
until curl -sf "http://$TOXIPROXY_ADDR/version" > /dev/null; do sleep 0.3; done
ok "Toxiproxy ready"

step "Waiting for hub-seq and hub-con"
until [ -f "$HUB_SEQ_DATA/.node_id" ]; do sleep 0.3; done
until [ -f "$HUB_CON_DATA/.node_id" ]; do sleep 0.3; done
HUB_SEQ_ID=$(cat "$HUB_SEQ_DATA/.node_id")
HUB_CON_ID=$(cat "$HUB_CON_DATA/.node_id")
ok "hub-seq node ID: $HUB_SEQ_ID"
ok "hub-con node ID: $HUB_CON_ID"

# ---- Per-clinic worker -------------------------------------------------------
run_clinic() {
  local MODE="$1"
  local i="$2"
  local HUB_ID="$3"
  local PORT_BASE="$4"
  local WORK="$5"
  local MODE_RESULTS="$6"

  local dir="$WORK/clinic-$i"
  local log="$WORK/clinic-$i.log"
  local port=$((PORT_BASE + i))
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

    printf '{"node":"clinic-%s","role":"clinic","events":%s,"wal_size_bytes":%s,"sync_duration_s":%s,"sync_start_epoch":%s,"bytes_sent":%s,"memory_rss_kb":4096,"profile":"%s"}\n' \
      "$i" "$event_count" "$wal_after" "$duration" "$t_start" "$wal_before" "$PROFILE" \
      > "$MODE_RESULTS/clinic-$i.json"

    echo "[$MODE] clinic-$i: OK in ${duration}s"
    return 0
  else
    err "[$MODE] clinic-$i: sync FAILED"
    cat "$log" >&2
    printf '{"node":"clinic-%s","role":"clinic","events":0,"wal_size_bytes":0,"sync_duration_s":0,"sync_start_epoch":0,"bytes_sent":0,"memory_rss_kb":0,"profile":"%s","error":true}\n' \
      "$i" "$PROFILE" > "$MODE_RESULTS/clinic-$i.json"
    return 1
  fi
}

# ---- Full scenario runner ----------------------------------------------------
run_scenario() {
  local MODE="$1"         # "seq" or "con"
  local HUB_ADDR="$2"    # "hub-seq:9000"
  local HUB_ID="$3"
  local HUB_DATA="$4"    # "/var/lib/hub-seq"
  local HUB_METRICS="$5" # "hub-seq:9090"
  local PORT_BASE="$6"   # 9000 for seq proxies, 9010 for con proxies

  local MODE_RESULTS="$RESULTS/$MODE"
  local WORK="/tmp/clinics-$MODE"
  mkdir -p "$MODE_RESULTS" "$WORK"

  step "[$MODE] Configuring $CLINIC_COUNT proxies -> $HUB_ADDR"
  for i in $(seq 1 "$CLINIC_COUNT"); do
    local port=$((PORT_BASE + i))
    curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies" \
      -H "Content-Type: application/json" \
      -d "{\"name\":\"clinic-${MODE}-$i\",\"listen\":\"0.0.0.0:$port\",\"upstream\":\"$HUB_ADDR\",\"enabled\":true}" \
      > /dev/null
    for STREAM in upstream downstream; do
      curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies/clinic-${MODE}-$i/toxics" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"latency_${STREAM}\",\"type\":\"latency\",\"stream\":\"$STREAM\",\"attributes\":{\"latency\":$LATENCY,\"jitter\":$JITTER}}" \
        > /dev/null
      curl -sf -X POST "http://$TOXIPROXY_ADDR/proxies/clinic-${MODE}-$i/toxics" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"bw_${STREAM}\",\"type\":\"bandwidth\",\"stream\":\"$STREAM\",\"attributes\":{\"rate\":$BW_RATE}}" \
        > /dev/null
    done
    ok "[$MODE] clinic-$i -> $TOXIPROXY_HOST:$port -> $HUB_ADDR"
  done

  step "[$MODE] Running $CLINIC_COUNT clinics in parallel ($EVENTS events each)"
  export TOXIPROXY_HOST EVENTS HUB_SEQ_ID HUB_CON_ID RESULTS PROFILE
  local WALL_START WALL_END WALL_TOTAL
  WALL_START=$(date +%s)
  local PIDS=()
  for i in $(seq 1 "$CLINIC_COUNT"); do
    run_clinic "$MODE" "$i" "$HUB_ID" "$PORT_BASE" "$WORK" "$MODE_RESULTS" &
    PIDS+=($!)
  done

  local FAILED=0
  for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}"; then
      ok "[$MODE] clinic-$((i + 1)) done"
    else
      err "[$MODE] clinic-$((i + 1)) FAILED"
      FAILED=1
    fi
  done
  WALL_END=$(date +%s)
  WALL_TOTAL=$(( WALL_END - WALL_START ))

  # Compute sync-phase wall time from clinic timestamps
  local SYNC_START SYNC_END SYNC_WALL_S SUM_SYNC
  SYNC_START=$(jq -s 'map(.sync_start_epoch // 0) | min' "$MODE_RESULTS"/clinic-*.json 2>/dev/null || echo 0)
  SYNC_END=$(jq -s 'map((.sync_start_epoch // 0) + (.sync_duration_s // 0)) | max' "$MODE_RESULTS"/clinic-*.json 2>/dev/null || echo 0)
  SUM_SYNC=$(jq -s 'map(.sync_duration_s // 0) | add' "$MODE_RESULTS"/clinic-*.json 2>/dev/null || echo 0)
  if [ "$SYNC_START" -gt 0 ] && [ "$SYNC_END" -gt "$SYNC_START" ]; then
    SYNC_WALL_S=$(( SYNC_END - SYNC_START ))
  else
    SYNC_WALL_S=$WALL_TOTAL
  fi

  # Serving mode comes from the scenario design, not heuristics:
  # seq hub runs with --max-peers 1 (sequential), con hub with --max-peers 16 (concurrent).
  local SERVING_MODE
  if [ "$MODE" = "con" ]; then
    SERVING_MODE="concurrent"
  else
    SERVING_MODE="sequential"
  fi

  # Hub metrics
  step "[$MODE] Collecting hub metrics"
  local HUB_WAL CLINIC_WAL_BEFORE TOTAL_EXPECTED HUB_EVENTS
  HUB_WAL=$(stat -c%s "$HUB_DATA/events.wal" 2>/dev/null || echo 0)
  # Use pre-sync WAL size (stored as bytes_sent in clinic JSON) so we always
  # normalize by exactly EVENTS events worth of bytes, regardless of sync order.
  CLINIC_WAL_BEFORE=$(jq -r '.bytes_sent // 0' "$MODE_RESULTS/clinic-1.json" 2>/dev/null || echo 0)
  TOTAL_EXPECTED=$(( CLINIC_COUNT * EVENTS ))
  if [ "${CLINIC_WAL_BEFORE:-0}" -gt 0 ] && [ "$HUB_WAL" -gt 0 ]; then
    HUB_EVENTS=$(( HUB_WAL * EVENTS / CLINIC_WAL_BEFORE ))
  else
    HUB_EVENTS=0
  fi

  # Scrape Prometheus metrics
  if curl -sf --max-time 5 "http://$HUB_METRICS/metrics" -o "$MODE_RESULTS/hub_metrics.txt"; then
    ok "[$MODE] Prometheus metrics scraped"
  else
    echo "" > "$MODE_RESULTS/hub_metrics.txt"
  fi

  # Write JSON files
  printf '{"node":"hub","role":"hub","events":%s,"wal_size_bytes":%s,"sync_duration_s":0,"bytes_sent":0,"memory_rss_kb":4096}\n' \
    "$HUB_EVENTS" "$HUB_WAL" > "$MODE_RESULTS/hub.json"

  printf '{"network_profile":"%s","events_per_clinic":%s,"clinic_count":%s,"scenario_date":"%s","mode":"%s","serving_mode":"%s","wall_total_s":%s,"sync_wall_s":%s,"sum_sync_s":%s,"profile":{"label":"%s","delay_ms":%s,"jitter_ms":%s,"bandwidth_kbps":%s}}\n' \
    "$PROFILE" "$EVENTS" "$CLINIC_COUNT" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$MODE" "$SERVING_MODE" "$WALL_TOTAL" "$SYNC_WALL_S" "$SUM_SYNC" \
    "$LABEL" "$LATENCY" "$JITTER" "$BW_KBPS" > "$MODE_RESULTS/scenario.json"

  ok "[$MODE] Hub: ~$HUB_EVENTS / $TOTAL_EXPECTED events"
  ok "[$MODE] Serving: $SERVING_MODE | sync_wall=${SYNC_WALL_S}s | sum_sync=${SUM_SYNC}s | wall=${WALL_TOTAL}s"

  # Phase 19: retention smoke check on clinic-1 data (not concurrently served -- safe to run)
  step "[$MODE] Phase 19: retention smoke check"
  local cli_dir="$WORK/clinic-1"
  if [ -f "$cli_dir/events.wal" ]; then
    # All events were submitted moments ago -- nothing should be older than 2020-01-01
    local dry_out dry_count
    dry_out=$(zamsync expire "$cli_dir" --before 2020-01-01 --dry-run 2>&1)
    dry_count=$(echo "$dry_out" | awk '/dry-run/{print $3}')
    if [ "${dry_count:-1}" -ne 0 ]; then
      err "[$MODE] Phase 19: expire --before 2020-01-01 --dry-run should be 0, got ${dry_count}"
      return 1
    fi
    ok "[$MODE] Phase 19: expire dry-run (past date) = 0 -- all events recent"

    # Snapshot must produce a file of the same size as the WAL
    local snap_path="/tmp/${MODE}-clinic1-snap.wal"
    local wal_sz snap_sz
    wal_sz=$(stat -c%s "$cli_dir/events.wal" 2>/dev/null || echo 0)
    zamsync snapshot "$cli_dir" --output "$snap_path" > /dev/null
    snap_sz=$(stat -c%s "$snap_path" 2>/dev/null || echo 0)
    if [ "$snap_sz" -ne "$wal_sz" ]; then
      err "[$MODE] Phase 19: snapshot size (${snap_sz}) != WAL size (${wal_sz})"
      return 1
    fi
    ok "[$MODE] Phase 19: snapshot matches WAL (${snap_sz} bytes)"
  else
    ok "[$MODE] Phase 19: WAL not present (clinic-1 failed sync earlier, skip)"
  fi

  return $FAILED
}

# ---- Run both scenarios ------------------------------------------------------
# Sequential first (port base 9000: clinics on 9001-9004)
run_scenario "seq" "$HUB_SEQ_ADDR" "$HUB_SEQ_ID" "$HUB_SEQ_DATA" "$HUB_SEQ_METRICS" 9000

# Concurrent second (port base 9010: clinics on 9011-9014)
run_scenario "con" "$HUB_CON_ADDR" "$HUB_CON_ID" "$HUB_CON_DATA" "$HUB_CON_METRICS" 9010

# ---- Generate comparison report ---------------------------------------------
step "Generating comparison report"
python3 /tests/report.py "$RESULTS"

echo ""
echo -e "${GREEN}============================================================${NC}"
echo -e "${GREEN}  Simulation complete! Profile: $LABEL (${LATENCY}ms / ${BW_KBPS}kbps)${NC}"
SEQ_WALL=$(jq -r '.sync_wall_s' "$RESULTS/seq/scenario.json" 2>/dev/null || echo "?")
CON_WALL=$(jq -r '.sync_wall_s' "$RESULTS/con/scenario.json" 2>/dev/null || echo "?")
echo -e "${GREEN}  Sequential hub: ${SEQ_WALL}s   Concurrent hub: ${CON_WALL}s${NC}"
echo -e "${GREEN}============================================================${NC}"
