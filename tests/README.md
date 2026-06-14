# ZamSync E2E Network Resilience Test (Bhutan 2G Simulation)

This test validates the resilience of the ZamSync synchronization engine under extremely degraded network conditions, typical of remote clinics in Bhutan.

It simulates a **2G satellite/cellular connection** with:
- **600ms latency**
- **100ms jitter**
- **50 kbps bandwidth limit (~6 KB/s)**
- **A sudden network blackout** (connection dropped mid-sync)

The test verifies that the client is able to resume syncing after network restoration and that no events are lost or duplicated.

---

## Architecture of the Test Suite

The test environment is orchestrated using Docker Compose and contains three containers:

```
                      +-------------------+
                      |   zamsync_tester  |  (Client Node)
                      |                   |
                      +---------+---------+
                                |  (Sync B -> A)
                                v  [Port 7000]
                      +-------------------+
                      | zamsync_toxiproxy |  (Simulates 2G Link)
                      |                   |
                      +---------+---------+
                                |
                                v  [Port 7000]
                      +-------------------+
                      |   zamsync_server  |  (Server Node / Hospital)
                      |                   |
                      +-------------------+
```

1. **`zamsync_server`**: Runs the server listening continuously on port 7000.
2. **`zamsync_toxiproxy`**: Uses Shopify's Toxiproxy to proxy port 7000 to the server with simulated latency, bandwidth, and connection control.
3. **`zamsync_tester`**: Generates 5,000 events offline, begins synchronization via the proxy, controls Toxiproxy to drop the connection mid-transfer, wait for failure, restore the connection, resume syncing, and verify that the server receives exactly 5,000 events.

---

## How to Run Locally

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [Docker Compose](https://docs.docker.com/compose/install/)

### Running the Test

Run the following command from the root of the project to build the containers and execute the test:

```bash
docker compose -f tests/docker-compose.test.yml up --build --abort-on-container-exit --force-recreate
```

* `--build` ensures any code changes in the repository are compiled into the test containers.
* `--abort-on-container-exit` causes the entire compose stack to shut down and return the exit code of the tester container once the script finishes (0 on success, 1 on failure).
* `--force-recreate` ensures fresh containers and anonymous volumes.

### Cleanup

To clean up test volumes and networks:

```bash
docker compose -f tests/docker-compose.test.yml down -v
```

---

## How to Run in CI (GitHub Actions)

The single-node Bhutan test is part of the standard CI workflow:

```yaml
  resilience-test:
    name: E2E Network Resilience Test (Bhutan 2G)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - run: docker compose -f tests/docker-compose.test.yml up --build --abort-on-container-exit --force-recreate
      - if: always()
        run: docker compose -f tests/docker-compose.test.yml down -v
```

---

## Multi-clinic Hospital Network Simulation

`docker-compose.network.yml` runs a full multi-node scenario: N clinics in parallel
over a Toxiproxy-throttled link to a hub, then generates an HTML report comparing
ZamSync against IPFS.

```bash
# Default: 4 clinics x 500 events, Bhutan 2G profile
docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit

# Report written to tests/results/report.html
start tests\results\report.html   # Windows
open  tests/results/report.html   # Linux
```

Override options:

```bash
PROFILE=satellite EVENTS=2000 CLINIC_COUNT=8 \
  docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit
```

| Profile | Latency | Bandwidth |
|---------|---------|-----------|
| `bhutan_2g` | 600ms ± 100ms | 30 kbps |
| `satellite` | 1200ms ± 200ms | 100 kbps |
| `urban_3g` | 80ms ± 20ms | 1 Mbps |

The full multi-clinic simulation is also run in CI via `.github/workflows/e2e-network.yml`
with `CLINIC_COUNT=2 EVENTS=100` for speed; the HTML report is uploaded as an artifact.
