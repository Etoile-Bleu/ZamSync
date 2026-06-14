#!/usr/bin/env bash
# ZamSync bootstrap -- run this from INSIDE the hub VM after `vagrant up`
#
#   vagrant ssh hub
#   cd /vagrant && bash scripts/bootstrap.sh
#
# This script:
#   1. Verifies Ansible is available on the hub
#   2. Waits for all clinic VMs to be reachable
#   3. Runs the full provision playbook (keygen, PKI, systemd)
#   4. Prints a summary

set -euo pipefail

VAGRANT_DIR=/vagrant
INVENTORY="$VAGRANT_DIR/ansible/inventory.ini"
PLAYBOOKS="$VAGRANT_DIR/ansible/playbooks"
SCRIPTS="$VAGRANT_DIR/ansible/scripts"
RESULTS="$VAGRANT_DIR/results"
CLINIC_COUNT="${CLINIC_COUNT:-4}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

step() { echo -e "\n${BLUE}==> $*${NC}"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
die()  { echo -e "${RED}[error]${NC} $*"; exit 1; }

# ---- 1. Check Ansible ---------------------------------------------------
step "Checking Ansible"
if ! command -v ansible-playbook &>/dev/null; then
  warn "Ansible not found, installing..."
  pip3 install -q ansible
fi
ok "$(ansible --version | head -1)"

# ---- 2. Fix SSH key ------------------------------------------------------
step "Setting up SSH key for inter-VM access"
KEY_CANDIDATES=(
  "/vagrant/.vagrant/machines/hub/virtualbox/private_key"
  "/vagrant/.vagrant/machines/clinic-1/virtualbox/private_key"
  "/etc/vagrant/insecure_private_key"
  "/home/vagrant/.vagrant.d/insecure_private_key"
)
KEY_DEST="/home/vagrant/.ssh/id_rsa"

if [[ ! -f "$KEY_DEST" ]]; then
  for candidate in "${KEY_CANDIDATES[@]}"; do
    if [[ -f "$candidate" ]]; then
      cp "$candidate" "$KEY_DEST"
      chmod 600 "$KEY_DEST"
      chown vagrant:vagrant "$KEY_DEST"
      ok "SSH key copied from $candidate"
      break
    fi
  done
fi

if [[ ! -f "$KEY_DEST" ]]; then
  warn "Could not find Vagrant private key. Downloading insecure key..."
  curl -fsSL \
    https://raw.githubusercontent.com/hashicorp/vagrant/main/keys/vagrant \
    -o "$KEY_DEST"
  chmod 600 "$KEY_DEST"
fi

# Update inventory with correct key path
sed -i "s|ansible_ssh_private_key_file=.*private_key|ansible_ssh_private_key_file=$KEY_DEST|g" \
  "$INVENTORY" 2>/dev/null || true

# ---- 3. Wait for clinic VMs to be reachable ------------------------------
step "Waiting for all clinic VMs to be reachable"
for i in $(seq 1 "$CLINIC_COUNT"); do
  IP="192.168.56.$((10 + i))"
  echo -n "  Waiting for clinic-$i ($IP)..."
  for attempt in $(seq 1 30); do
    if ssh -i "$KEY_DEST" -o ConnectTimeout=2 -o BatchMode=yes \
       vagrant@"$IP" "true" 2>/dev/null; then
      echo -e " ${GREEN}ready${NC}"
      break
    fi
    if [[ $attempt -eq 30 ]]; then
      die "clinic-$i ($IP) unreachable after 30 attempts. Is it running? (vagrant up clinic-$i)"
    fi
    echo -n "."
    sleep 2
  done
done

# ---- 4. Run provision playbook -------------------------------------------
step "Running provision playbook (keygen, PKI, systemd)"
ansible-playbook \
  -i "$INVENTORY" \
  "$PLAYBOOKS/provision.yml" \
  -e "zamsync_version=${ZAMSYNC_VERSION:-1.0.3}" \
  -e "clinic_count=$CLINIC_COUNT" \
  --private-key="$KEY_DEST" \
  -v

# ---- 5. Done -------------------------------------------------------------
echo ""
echo -e "${GREEN}============================================================${NC}"
echo -e "${GREEN}  ZamSync cluster provisioned successfully!${NC}"
echo -e "${GREEN}============================================================${NC}"
echo ""
echo "  Hub:      192.168.56.10:9000 (mTLS)"
for i in $(seq 1 "$CLINIC_COUNT"); do
  echo "  clinic-$i: 192.168.56.$((10 + i)):9000"
done
echo ""
echo "Next steps:"
echo "  # Run the Bhutan 2G scenario"
echo "  bash scripts/scenario.sh"
echo ""
echo "  # Or run individual playbooks:"
echo "  ansible-playbook -i ansible/inventory.ini ansible/playbooks/degrade-network.yml"
echo "  ansible-playbook -i ansible/inventory.ini ansible/playbooks/scenario.yml"
