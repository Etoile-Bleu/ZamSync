# ZamSync Hospital Network Simulation

Vagrant + Ansible environment that spins up a realistic multi-clinic deployment
and measures ZamSync performance under simulated 2G / satellite / 3G network
conditions.

## Architecture

```
192.168.56.0/24 (VirtualBox private network)

  hub          192.168.56.10   512 MB   full speed
  clinic-1     192.168.56.11   512 MB   tc netem (configurable profile)
  clinic-2     192.168.56.12   512 MB   tc netem
  clinic-3     192.168.56.13   512 MB   tc netem
  clinic-4     192.168.56.14   512 MB   tc netem
```

Each VM is Ubuntu 22.04 LTS with 512 MB RAM -- matching Raspberry Pi 3 class
hardware used in rural health facilities.

## Prerequisites

- [VirtualBox](https://www.virtualbox.org/) >= 7.0
- [Vagrant](https://www.vagrantup.com/) >= 2.3

Ansible runs **inside the hub VM** -- no installation needed on the host.

## Quick Start

### Windows (PowerShell)

Ansible does not run natively on Windows. The hub VM acts as the Ansible
control node instead.

```powershell
cd vagrant\

# Boot all VMs (~5 min on first run)
vagrant up

# SSH into the hub
vagrant ssh hub

# Inside the hub VM -- provision everything from here
cd /vagrant
bash scripts/bootstrap.sh

# Run the Bhutan 2G scenario (still inside hub)
bash scripts/scenario.sh

# Back on Windows -- open the report
start C:\dev\ZamSync\vagrant\results\report.html
```

### Linux / macOS (Ansible on host)

```bash
pip install ansible
cd vagrant/
vagrant up
ansible-playbook -i ansible/inventory.ini ansible/playbooks/provision.yml
ansible-playbook -i ansible/inventory.ini ansible/playbooks/scenario.yml
open results/report.html
```

## Network Profiles

| Profile | Latency | Bandwidth | Packet loss | Scenario |
|---------|---------|-----------|-------------|----------|
| `bhutan_2g` (default) | 600ms +/- 100ms | 30 kbps | 5% | Rural clinic, 2G/EDGE |
| `satellite` | 1200ms +/- 200ms | 100 kbps | 2% | Very remote, VSAT |
| `urban_3g` | 80ms +/- 20ms | 1 Mbps | 0.1% | Urban 3G baseline |

Override at runtime:

```bash
# Run scenario with satellite profile
ansible-playbook -i ansible/inventory.ini ansible/playbooks/scenario.yml \
  -e active_network_profile=satellite

# Increase event count
ansible-playbook -i ansible/inventory.ini ansible/playbooks/scenario.yml \
  -e events_per_clinic=2000
```

## Scaling

Change the number of clinic VMs with `CLINIC_COUNT`:

```bash
CLINIC_COUNT=8 vagrant up
ansible-playbook -i ansible/inventory.ini ansible/playbooks/provision.yml
ansible-playbook -i ansible/inventory.ini ansible/playbooks/scenario.yml
```

> Note: update `ansible/inventory.ini` to add the extra clinic hosts when scaling
> beyond 4 (they are pre-defined up to `clinic-8`).

## Individual Playbooks

```bash
# Apply degradation only (without running full scenario)
ansible-playbook -i ansible/inventory.ini ansible/playbooks/degrade-network.yml

# Restore network to full speed
ansible-playbook -i ansible/inventory.ini ansible/playbooks/restore-network.yml

# Re-generate the HTML report from existing results
python3 ansible/scripts/report.py results/
```

## Scenario Phases

| Phase | What happens |
|-------|-------------|
| **0 -- Reset** | Stop services, clear WALs (keep TLS certs) |
| **1 -- Offline** | Clinics submit N patient events with no hub reachable |
| **2 -- Degrade** | Apply tc netem on clinic network interfaces |
| **3 -- Sync** | Hub starts; all clinics sync simultaneously over degraded link |
| **4 -- Verify** | Check hub has received all events (convergence) |
| **5 -- Collect** | Gather metrics: event counts, WAL size, RSS, bytes sent, sync time |
| **6 -- Report** | Generate self-contained HTML report with Chart.js graphs |

## Report Contents

The generated `results/report.html` includes:

- **KPI dashboard** -- convergence %, total events, avg sync time, bytes transferred, memory
- **Sync duration chart** -- per-clinic bar chart
- **Bandwidth chart** -- ZamSync actual bytes vs IPFS estimated overhead for same workload
- **Memory chart** -- ZamSync RSS vs IPFS daemon RSS
- **Per-event overhead chart** -- ZamSync 21-byte WAL header vs IPFS 256+ byte block
- **Feature comparison table** -- ZamSync vs IPFS: mTLS, encryption, access control,
  deterministic ordering, offline-first, ARM support, single binary

## ZamSync vs IPFS -- Why This Matters

IPFS is sometimes proposed for offline-first health data scenarios. The simulation
exposes why IPFS is ill-suited for constrained hospital networks:

| | ZamSync | IPFS (Kubo) |
|---|---|---|
| **RAM** | ~4 MB | ~150--500 MB |
| **Per-event wire overhead** | 21 bytes | 256+ bytes (block + CID) |
| **mTLS mutual auth** | Built-in | Not native |
| **Encryption at rest** | ChaCha20-Poly1305 | None built-in |
| **Role-based isolation** | `--policy own` | None |
| **Offline accumulation** | WAL grows forever offline | Requires pinning + peers |
| **Sync efficiency** | Sends exactly missing events (VV diff) | Gossip / Bitswap can over-fetch |
| **Deterministic ordering** | HLC + Version Vectors | No |
| **Single binary** | Yes (< 5 MB, musl) | No (Go daemon + config dir) |
| **ARM64 / ARMv7** | Yes (CI-built) | Limited |

## Troubleshooting

**VMs not starting** -- check VirtualBox host-only adapter is enabled.

**Ansible SSH fails** -- run `vagrant ssh-config` and verify key paths match
`ansible/inventory.ini`.

**tc netem missing** -- already installed by the initial shell provisioner.
If not: `sudo apt-get install -y iproute2` on the clinic VM.

**ZamSync binary not found** -- check GitHub release URL for the resolved version,
or pin a specific version with `-e zamsync_version=1.0.3`.
