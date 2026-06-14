#!/usr/bin/env bash
#
# E2E Security Test: verifies that ZamSync rejects unauthorized clients
# at the mTLS level (no valid certificate = no connection), and that
# the OwnOnly access policy prevents clinic A from reading clinic B's data.
#
set -euo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BLUE}======================================================================${NC}"
echo -e "${BOLD} Starting ZamSync E2E Security & Access Control Test${NC}"
echo -e "${BLUE}======================================================================${NC}"

PASS_COUNT=0
FAIL_COUNT=0

pass() { echo -e "${GREEN}[PERFECT]${NC} $1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { echo -e "${RED}[CRITICAL]${NC} $1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
info() { echo -e "${BLUE}[INFO]${NC} $1"; }

# ---------------------------------------------------------------------------
# SETUP: generate PKI for the "trusted cluster"
# ---------------------------------------------------------------------------
info "Generating trusted cluster credentials (Hospital + Clinic A + Clinic B)..."

rm -rf /data/hospital /data/clinic_a /data/clinic_b /data/rogue

zamsync info /data/hospital > /dev/null
zamsync info /data/clinic_a > /dev/null
zamsync info /data/clinic_b > /dev/null
zamsync info /data/rogue    > /dev/null

HOSPITAL_ID=$(cat /data/hospital/.node_id)
CLINIC_A_ID=$(cat /data/clinic_a/.node_id)
CLINIC_B_ID=$(cat /data/clinic_b/.node_id)
ROGUE_ID=$(cat /data/rogue/.node_id)

info "Hospital Node ID : ${CYAN}$HOSPITAL_ID${NC}"
info "Clinic A  Node ID: ${CYAN}$CLINIC_A_ID${NC}"
info "Clinic B  Node ID: ${CYAN}$CLINIC_B_ID${NC}"
info "Rogue     Node ID: ${CYAN}$ROGUE_ID${NC}"

# Generate trusted PKI: hospital keygen creates CA + node cert for the cluster
zamsync keygen /data/hospital
# Clinics share the hospital's CA: copy ca.crt and generate their own node certs
# (Simplified: all nodes use the same hospital CA)
# Real deployment: use `ca.key` to sign individual node certs
mkdir -p /data/clinic_a/tls /data/clinic_b/tls /data/rogue/tls
cp /data/hospital/tls/ca.crt /data/clinic_a/tls/ca.crt
cp /data/hospital/tls/ca.crt /data/clinic_b/tls/ca.crt
# Clinic A and B get their own node cert from hospital CA
# For the test, we let each run keygen (self-signed CA) - rogue gets NO hospital CA
zamsync keygen /data/clinic_a
zamsync keygen /data/clinic_b
# Rogue: generate its own self-signed CA (completely different PKI)
zamsync keygen /data/rogue

# Hospital and clinics: use the hospital's CA to validate each other
# Copy hospital's CA as the trust anchor for clinic_a and clinic_b
cp /data/hospital/tls/ca.crt /data/clinic_a/tls/ca.crt
cp /data/hospital/tls/ca.crt /data/clinic_b/tls/ca.crt
# Copy clinic node certs signed under hospital CA (self-keygen - workaround for test environment)
# In production: hospital CA would sign clinic certs. Here, each clinic has its own CA cert.
# We test the rogue scenario: rogue has a different CA entirely.

# ---------------------------------------------------------------------------
# TEST 1: OwnOnly Policy - Clinic A CANNOT read Clinic B's events
# ---------------------------------------------------------------------------
echo ""
echo -e "${YELLOW}=== TEST 1: OwnOnly Access Control Policy ===${NC}"
info "Submitting patient records for Clinic A and Clinic B..."
zamsync submit /data/clinic_a "clinic-a-patient-record-001" > /dev/null
zamsync submit /data/clinic_a "clinic-a-patient-record-002" > /dev/null
zamsync submit /data/clinic_b "clinic-b-patient-record-001" > /dev/null
zamsync submit /data/clinic_b "clinic-b-patient-record-002" > /dev/null

# Start hospital hub in OwnOnly mode in background (plain TCP for this sub-test)
HOSPITAL_PORT=17001
zamsync serve /data/hospital 127.0.0.1:$HOSPITAL_PORT --policy own &
HOSPITAL_PID=$!
sleep 0.5

# Clinic A syncs its data to hospital
zamsync sync /data/clinic_a 127.0.0.1:$HOSPITAL_PORT $HOSPITAL_ID > /dev/null 2>&1 || true
sleep 0.2
# Clinic B syncs its data to hospital
zamsync sync /data/clinic_b 127.0.0.1:$HOSPITAL_PORT $HOSPITAL_ID > /dev/null 2>&1 || true
sleep 0.2

# Verify hospital has all 4 events
HOSPITAL_EVENTS=$(zamsync info /data/hospital | grep "events" | awk '{print $3}')
if [ "$HOSPITAL_EVENTS" -eq 4 ]; then
  pass "Hospital received all 4 events (2 from A + 2 from B)"
else
  fail "Hospital has $HOSPITAL_EVENTS events instead of 4"
fi

# Kill hospital, restart in OwnOnly mode with fresh clinic_a data dir to test retrieval
kill $HOSPITAL_PID 2>/dev/null || true
sleep 0.3

# Restart hospital in OwnOnly mode to test data retrieval
HOSPITAL_PORT=17002
rm -rf /data/clinic_a_fresh
zamsync info /data/clinic_a_fresh > /dev/null
# Fresh clinic A pulls from hospital: must only get its OWN events
zamsync serve /data/hospital 127.0.0.1:$HOSPITAL_PORT --policy own &
HOSPITAL_PID=$!
sleep 0.5

# Ensure clinic_a_fresh uses clinic_a's NodeId so the hospital OwnOnly policy works
echo "$CLINIC_A_ID" > /data/clinic_a_fresh/.node_id
zamsync sync /data/clinic_a_fresh 127.0.0.1:$HOSPITAL_PORT $HOSPITAL_ID > /dev/null 2>&1 || true
sleep 0.2

kill $HOSPITAL_PID 2>/dev/null || true

FRESH_A_EVENTS=$(zamsync info /data/clinic_a_fresh | grep "events" | awk '{print $3}')
if [ "$FRESH_A_EVENTS" -eq 2 ]; then
  pass "OwnOnly policy: Clinic A received only its 2 events (Clinic B's data is isolated)"
else
  fail "OwnOnly policy: Clinic A has $FRESH_A_EVENTS events instead of 2 (ISOLATION BROKEN)"
fi

# ---------------------------------------------------------------------------
# TEST 2: mTLS - Rogue client with unknown CA is rejected at TLS handshake
# ---------------------------------------------------------------------------
echo ""
echo -e "${YELLOW}=== TEST 2: mTLS Authentication - Rogue Client Rejection ===${NC}"
info "Starting Hospital TLS server (mTLS enabled)..."

TLS_PORT=17003
zamsync serve /data/hospital 127.0.0.1:$TLS_PORT --tls &
HOSPITAL_TLS_PID=$!
sleep 0.5

# Rogue client attempts to connect using its own self-signed CA (different PKI)
info "Rogue client attempting mTLS connection to Hospital..."
set +e
zamsync sync /data/rogue 127.0.0.1:$TLS_PORT $HOSPITAL_ID --tls > /tmp/rogue_sync.log 2>&1
ROGUE_EXIT=$?
set -e

if [ $ROGUE_EXIT -ne 0 ]; then
  pass "Rogue client was REJECTED (exit code $ROGUE_EXIT)"
  ROGUE_LOG=$(cat /tmp/rogue_sync.log | head -3)
  info "Rejection reason: $ROGUE_LOG"
else
  fail "SECURITY BREACH: Rogue client connected successfully!"
fi

# Verify hospital received ZERO events from the rogue client
HOSPITAL_EVENTS_AFTER_ROGUE=$(zamsync info /data/hospital | grep "events" | awk '{print $3}')
if [ "$HOSPITAL_EVENTS_AFTER_ROGUE" -eq 4 ]; then
  pass "Hospital event count unchanged ($HOSPITAL_EVENTS_AFTER_ROGUE) - rogue data did not leak in"
else
  fail "Hospital event count changed to $HOSPITAL_EVENTS_AFTER_ROGUE - potential injection!"
fi

kill $HOSPITAL_TLS_PID 2>/dev/null || true

# ---------------------------------------------------------------------------
# TEST 3: Plain TCP connection attempt to a TLS-only server is rejected
# ---------------------------------------------------------------------------
echo ""
echo -e "${YELLOW}=== TEST 3: Plain TCP against TLS Server is Rejected ===${NC}"
info "Starting Hospital TLS server again..."

TLS_PORT=17004
zamsync serve /data/hospital 127.0.0.1:$TLS_PORT --tls &
HOSPITAL_TLS_PID2=$!
sleep 0.5

info "Plain TCP client attempting connection to TLS-only server..."
set +e
# Clinic A (valid credentials, but using plain TCP / no --tls flag)
zamsync sync /data/clinic_a 127.0.0.1:$TLS_PORT $HOSPITAL_ID > /tmp/plain_sync.log 2>&1
PLAIN_EXIT=$?
set -e

if [ $PLAIN_EXIT -ne 0 ]; then
  pass "Plain TCP client rejected by TLS server (exit code $PLAIN_EXIT)"
else
  fail "SECURITY ISSUE: Plain TCP client connected to TLS-only server!"
fi

kill $HOSPITAL_TLS_PID2 2>/dev/null || true

# ---------------------------------------------------------------------------
# RESULTS
# ---------------------------------------------------------------------------
echo ""
echo -e "${BLUE}======================================================================${NC}"
echo -e "${BOLD}                   ZAMSYNC SECURITY TEST RESULTS                      ${NC}"
echo -e "${BLUE}======================================================================${NC}"
echo -e " * OwnOnly Access Policy:       $([ $PASS_COUNT -ge 2 ] && echo -e "${GREEN}PERFECT${NC}" || echo -e "${RED}CRITICAL${NC}")"
echo -e " * mTLS Rogue Client Rejection: $([ $PASS_COUNT -ge 3 ] && echo -e "${GREEN}PERFECT${NC}" || echo -e "${RED}CRITICAL${NC}")"
echo -e " * Plain TCP vs TLS Server:     $([ $PASS_COUNT -ge 4 ] && echo -e "${GREEN}PERFECT${NC}" || echo -e "${RED}CRITICAL${NC}")"
echo -e " Tests passed: ${GREEN}$PASS_COUNT${NC}  |  Tests failed: $([ $FAIL_COUNT -eq 0 ] && echo -e "${GREEN}$FAIL_COUNT${NC}" || echo -e "${RED}$FAIL_COUNT${NC}")"
echo -e "${BLUE}======================================================================${NC}"

if [ $FAIL_COUNT -gt 0 ]; then
  echo -e "${RED}[CRITICAL]${NC} $FAIL_COUNT security test(s) failed!"
  exit 1
fi

echo -e "${GREEN}[SUCCESS]${NC} All security tests passed. ZamSync correctly rejects unauthorized access."
echo -e "${BLUE}======================================================================${NC}"
