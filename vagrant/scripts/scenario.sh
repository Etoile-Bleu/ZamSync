#!/usr/bin/env bash
# Run the hospital network scenario from inside the hub VM.
#
#   vagrant ssh hub
#   cd /vagrant && bash scripts/scenario.sh [OPTIONS]
#
# Options (env vars):
#   PROFILE=bhutan_2g|satellite|urban_3g   (default: bhutan_2g)
#   EVENTS=500                              (default: 500)
#   CLINIC_COUNT=4                          (default: 4)
#
# Examples:
#   bash scripts/scenario.sh
#   PROFILE=satellite EVENTS=2000 bash scripts/scenario.sh

set -euo pipefail

VAGRANT_DIR=/vagrant
INVENTORY="$VAGRANT_DIR/ansible/inventory.ini"
PLAYBOOKS="$VAGRANT_DIR/ansible/playbooks"
KEY="/home/vagrant/.ssh/id_rsa"
RESULTS="$VAGRANT_DIR/results"
PROFILE="${PROFILE:-bhutan_2g}"
EVENTS="${EVENTS:-500}"

BLUE='\033[0;34m'; GREEN='\033[0;32m'; NC='\033[0m'
step() { echo -e "\n${BLUE}==> $*${NC}"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }

mkdir -p "$RESULTS"

ANSIBLE_ARGS=(
  -i "$INVENTORY"
  --private-key="$KEY"
  -e "active_network_profile=$PROFILE"
  -e "events_per_clinic=$EVENTS"
)

step "Profile: $PROFILE | Events per clinic: $EVENTS"

step "Running scenario"
ansible-playbook "${ANSIBLE_ARGS[@]}" "$PLAYBOOKS/scenario.yml"

step "Report generated"
ok "Open the report: $RESULTS/report.html"
echo ""
echo "  On Windows (from your host):"
echo "  start C:\\dev\\ZamSync\\vagrant\\results\\report.html"
