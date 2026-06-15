#!/usr/bin/env python3
"""
ZamSync Hospital Network Simulation -- Benchmark Report Generator

If <results-dir>/seq/ and <results-dir>/con/ both exist, generates a
side-by-side comparison report (sequential vs concurrent hub).
Otherwise generates a single-run report.

Usage:
    python3 report.py <results-dir>
"""

import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

IPFS_COMPARISON = {
    "memory_mb": 210,
    "bytes_per_event": 612,
    "min_ram_mb": 150,
}

ZAMSYNC_FACTS = {
    "bytes_per_event": 125,
    "min_ram_mb": 4,
}


# ---- Prometheus text format parser ------------------------------------------

def parse_prometheus(text: str) -> dict:
    result = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = re.match(
            r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{([^}]*)\})?\s+([\d.e+\-Ee]+(?:inf)?)',
            line, re.IGNORECASE,
        )
        if not m:
            continue
        name = m.group(1)
        labels_str = m.group(2) or ""
        try:
            value = float(m.group(3))
        except ValueError:
            continue
        labels = {}
        for lm in re.finditer(r'(\w+)="([^"]*)"', labels_str):
            labels[lm.group(1)] = lm.group(2)
        result[(name, frozenset(labels.items()))] = value
    return result


def prom_get(prom: dict, name: str, **filters) -> float | None:
    for (n, lf), v in prom.items():
        if n != name:
            continue
        labels = dict(lf)
        if all(labels.get(k) == str(fv) for k, fv in filters.items()):
            return v
    return None


def prom_all(prom: dict, name: str, label: str) -> dict:
    out = {}
    for (n, lf), v in prom.items():
        if n != name:
            continue
        labels = dict(lf)
        if label in labels:
            out[labels[label]] = v
    return out


# ---- Data loading -----------------------------------------------------------

def load_scenario(path: Path):
    scenario, nodes, prom = {}, [], {}
    meta = path / "scenario.json"
    if meta.exists():
        scenario = json.loads(meta.read_text())
    for f in sorted(path.glob("*.json")):
        if f.name == "scenario.json":
            continue
        try:
            nodes.append(json.loads(f.read_text()))
        except json.JSONDecodeError:
            pass
    metrics_txt = path / "hub_metrics.txt"
    if metrics_txt.exists():
        prom = parse_prometheus(metrics_txt.read_text())
    return scenario, nodes, prom


# ---- CSS / JS shared blocks -------------------------------------------------

CSS = """
  :root {
    --bg: #0f1117; --card: #1a1d27; --border: #2a2d3a;
    --text: #e2e8f0; --muted: #94a3b8; --accent: #6366f1;
    --green: #22c55e; --red: #ef4444; --yellow: #eab308;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { background: var(--bg); color: var(--text); font-family: 'Segoe UI', system-ui, sans-serif; padding: 2rem; }
  h1 { font-size: 1.75rem; font-weight: 700; color: var(--accent); margin-bottom: 0.25rem; }
  h2 { font-size: 1.1rem; font-weight: 600; color: var(--text); margin-bottom: 1rem; border-bottom: 1px solid var(--border); padding-bottom: 0.5rem; }
  .subtitle { color: var(--muted); font-size: 0.9rem; margin-bottom: 2rem; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; margin-bottom: 2rem; }
  .kpi { background: var(--card); border: 1px solid var(--border); border-radius: 0.75rem; padding: 1.25rem; }
  .kpi-value { font-size: 2rem; font-weight: 700; color: var(--accent); }
  .kpi-label { font-size: 0.8rem; color: var(--muted); margin-top: 0.25rem; text-transform: uppercase; letter-spacing: 0.05em; }
  .kpi-ok .kpi-value { color: var(--green); }
  .kpi-warn .kpi-value { color: var(--yellow); }
  .kpi-accent .kpi-value { color: var(--accent); }
  .charts { display: grid; grid-template-columns: repeat(auto-fit, minmax(480px, 1fr)); gap: 1.5rem; margin-bottom: 2rem; }
  .chart-card { background: var(--card); border: 1px solid var(--border); border-radius: 0.75rem; padding: 1.5rem; }
  .chart-card.wide { grid-column: 1 / -1; }
  table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  th { background: var(--border); padding: 0.6rem 1rem; text-align: left; color: var(--muted); font-weight: 600; font-size: 0.75rem; text-transform: uppercase; }
  td { padding: 0.6rem 1rem; border-bottom: 1px solid var(--border); }
  .yes { color: var(--green); font-weight: 600; }
  .no { color: var(--red); }
  .badge { display: inline-block; padding: 0.15rem 0.5rem; border-radius: 9999px; font-size: 0.75rem; font-weight: 600; }
  .badge-ok { background: rgba(34,197,94,0.15); color: var(--green); }
  .badge-warn { background: rgba(234,179,8,0.15); color: var(--yellow); }
  .badge-accent { background: rgba(99,102,241,0.15); color: var(--accent); }
  .section { margin-bottom: 2.5rem; }
  .hero { display: flex; align-items: center; gap: 2rem; background: var(--card); border: 1px solid var(--border); border-radius: 0.75rem; padding: 2rem; margin-bottom: 2rem; }
  .hero-speedup { font-size: 5rem; font-weight: 800; color: var(--green); line-height: 1; }
  .hero-label { color: var(--muted); font-size: 0.9rem; margin-top: 0.5rem; }
  .hero-detail { color: var(--text); font-size: 1rem; margin-top: 0.25rem; }
  footer { color: var(--muted); font-size: 0.8rem; margin-top: 3rem; padding-top: 1rem; border-top: 1px solid var(--border); }
"""

JS_COLORS = """
const C = {
  seq:    'rgba(239, 68,  68,  0.8)',
  con:    'rgba(99,  102, 241, 0.8)',
  seq_b:  'rgba(239, 68,  68,  1)',
  con_b:  'rgba(99,  102, 241, 1)',
  ipfs:   'rgba(234, 179, 8,   0.7)',
  ipfs_b: 'rgba(234, 179, 8,   1)',
};
const base = {
  plugins: { legend: { labels: { color: '#94a3b8' } } },
  scales: {
    x: { ticks: { color: '#94a3b8' }, grid: { color: '#2a2d3a' } },
    y: { ticks: { color: '#94a3b8' }, grid: { color: '#2a2d3a' } }
  }
};
"""

FEATURE_TABLE = """
<div class="section">
  <h2>ZamSync vs IPFS -- Feature Comparison</h2>
  <table>
    <thead><tr><th>Feature</th><th>ZamSync</th><th>IPFS (Kubo)</th><th>Notes</th></tr></thead>
    <tbody>
      <tr><td>Mutual TLS (mTLS)</td><td><span class="yes">Yes</span></td><td><span class="no">No</span></td><td>IPFS uses libp2p noise, no client cert auth</td></tr>
      <tr><td>Encryption at rest</td><td><span class="yes">Yes (ChaCha20-Poly1305)</span></td><td><span class="no">No</span></td><td>ZamSync encrypts WAL records; IPFS stores plaintext blocks</td></tr>
      <tr><td>Role-based access control</td><td><span class="yes">Yes (--policy own)</span></td><td><span class="no">No</span></td><td>ZamSync enforces per-clinic isolation</td></tr>
      <tr><td>Deterministic event ordering</td><td><span class="yes">Yes (HLC + Version Vectors)</span></td><td><span class="no">No</span></td><td>IPFS is content-addressed DAG; no built-in total order</td></tr>
      <tr><td>Min RAM footprint</td><td><span class="yes">~4 MB</span></td><td><span class="no">~150 MB</span></td><td>ZamSync targets RPi class (512 MB)</td></tr>
      <tr><td>WAL record overhead</td><td><span class="yes">~125 bytes/record</span></td><td><span class="no">612+ bytes/block</span></td><td>VV diff sends only missing events; IPFS gossip is redundant</td></tr>
      <tr><td>Native offline-first sync</td><td><span class="yes">Yes</span></td><td><span class="badge badge-warn">Partial</span></td><td>IPFS needs peers online; ZamSync WAL accumulates offline</td></tr>
      <tr><td>Single static binary</td><td><span class="yes">Yes (&lt;5 MB)</span></td><td><span class="no">No (Go daemon)</span></td><td>ZamSync ships musl-linked; IPFS needs Go runtime</td></tr>
      <tr><td>ARM64 / ARMv7 support</td><td><span class="yes">Yes (cross-compiled)</span></td><td><span class="badge badge-warn">Limited</span></td><td>ZamSync CI builds for aarch64 + armv7</td></tr>
    </tbody>
  </table>
</div>
"""


# ---- Comparison report (seq + con) ------------------------------------------

def make_comparison_report(results_dir: Path):
    seq_sc, seq_nodes, seq_prom = load_scenario(results_dir / "seq")
    con_sc, con_nodes, con_prom = load_scenario(results_dir / "con")

    seq_clinics = [n for n in seq_nodes if n.get("role") == "clinic"]
    con_clinics = [n for n in con_nodes if n.get("role") == "clinic"]
    profile = seq_sc.get("profile", con_sc.get("profile", {}))

    seq_wall = seq_sc.get("sync_wall_s", seq_sc.get("wall_total_s", 0))
    con_wall = con_sc.get("sync_wall_s", con_sc.get("wall_total_s", 0))
    seq_sum  = seq_sc.get("sum_sync_s", sum(c.get("sync_duration_s", 0) for c in seq_clinics))
    con_sum  = con_sc.get("sum_sync_s", sum(c.get("sync_duration_s", 0) for c in con_clinics))

    speedup = round(seq_wall / con_wall, 1) if con_wall > 0 else 0

    # Hub-side session data from Prometheus
    seq_session_sum   = prom_get(seq_prom, "zamsync_sync_duration_seconds_sum", role="responder") or 0
    seq_session_count = int(prom_get(seq_prom, "zamsync_sync_duration_seconds_count", role="responder") or 0)
    con_session_sum   = prom_get(con_prom, "zamsync_sync_duration_seconds_sum", role="responder") or 0
    con_session_count = int(prom_get(con_prom, "zamsync_sync_duration_seconds_count", role="responder") or 0)
    seq_avg = seq_session_sum / seq_session_count if seq_session_count > 0 else 0
    con_avg = con_session_sum / con_session_count if con_session_count > 0 else 0

    # Hub event counts
    seq_hub = next((n for n in seq_nodes if n.get("role") == "hub"), {})
    con_hub = next((n for n in con_nodes if n.get("role") == "hub"), {})
    n_clinics = len(seq_clinics)
    total_expected = seq_sc.get("events_per_clinic", 500) * n_clinics

    # Per-clinic sync times for grouped chart
    clinic_labels = [f"clinic-{i+1}" for i in range(n_clinics)]
    seq_times = [seq_clinics[i].get("sync_duration_s", 0) if i < len(seq_clinics) else 0 for i in range(n_clinics)]
    con_times = [con_clinics[i].get("sync_duration_s", 0) if i < len(con_clinics) else 0 for i in range(n_clinics)]

    # Summary quantiles from Prometheus
    seq_quantiles = {
        str(q): prom_get(seq_prom, "zamsync_sync_duration_seconds", role="responder", quantile=str(q))
        for q in ["0", "0.5", "0.9", "0.99", "1"]
    }
    con_quantiles = {
        str(q): prom_get(con_prom, "zamsync_sync_duration_seconds", role="responder", quantile=str(q))
        for q in ["0", "0.5", "0.9", "0.99", "1"]
    }
    has_quantiles = any(v is not None for v in seq_quantiles.values())

    quantile_labels = ["min (q0)", "p50", "p90", "p99", "max (q1)"]
    seq_q_values = [round(seq_quantiles.get(q) or 0, 3) for q in ["0", "0.5", "0.9", "0.99", "1"]]
    con_q_values = [round(con_quantiles.get(q) or 0, 3) for q in ["0", "0.5", "0.9", "0.99", "1"]]

    # Bytes comparison
    seq_bytes = sum(c.get("bytes_sent", 0) for c in seq_clinics)
    con_bytes = sum(c.get("bytes_sent", 0) for c in con_clinics)
    ipfs_bytes = int(seq_sc.get("events_per_clinic", 500) * IPFS_COMPARISON["bytes_per_event"]) * n_clinics

    run_date = seq_sc.get("scenario_date", datetime.now(timezone.utc).isoformat())

    quantile_chart = ""
    if has_quantiles:
        quantile_chart = f"""
    <div class="chart-card">
      <h2>Hub Session Duration Distribution (Prometheus quantiles)</h2>
      <canvas id="quantileChart"></canvas>
    </div>"""

    quantile_js = ""
    if has_quantiles:
        quantile_js = f"""
new Chart(document.getElementById('quantileChart'), {{
  type: 'bar',
  data: {{
    labels: {json.dumps(quantile_labels)},
    datasets: [
      {{ label: 'Sequential (hub-side, s)', data: {json.dumps(seq_q_values)}, backgroundColor: C.seq, borderColor: C.seq_b, borderWidth: 1 }},
      {{ label: 'Concurrent (hub-side, s)', data: {json.dumps(con_q_values)}, backgroundColor: C.con, borderColor: C.con_b, borderWidth: 1 }}
    ]
  }},
  options: base
}});"""

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ZamSync -- Sequential vs Concurrent Hub Benchmark</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
<style>{CSS}</style>
</head>
<body>

<h1>ZamSync -- Sequential vs Concurrent Hub</h1>
<p class="subtitle">
  Profile: {profile.get("label", "")} &mdash;
  {profile.get("delay_ms", "?")}ms latency / {profile.get("bandwidth_kbps", "?")}kbps &mdash;
  {n_clinics} clinics &times; {seq_sc.get("events_per_clinic", 500)} events &mdash;
  Run: {run_date}
</p>

<div class="section">
  <div class="hero">
    <div>
      <div class="hero-speedup">{speedup}x</div>
      <div class="hero-label">Concurrent Speedup (Phase 14)</div>
      <div class="hero-detail">Sync wall time: {seq_wall}s &rarr; {con_wall}s for {n_clinics} simultaneous clinics</div>
    </div>
    <div style="flex:1">
      <div class="grid" style="margin:0">
        <div class="kpi kpi-warn">
          <div class="kpi-value">{seq_wall}s</div>
          <div class="kpi-label">Sequential wall time</div>
        </div>
        <div class="kpi kpi-ok">
          <div class="kpi-value">{con_wall}s</div>
          <div class="kpi-label">Concurrent wall time</div>
        </div>
        <div class="kpi">
          <div class="kpi-value">{seq_sum}s</div>
          <div class="kpi-label">Sum session time (seq)</div>
        </div>
        <div class="kpi">
          <div class="kpi-value">{con_sum}s</div>
          <div class="kpi-label">Sum session time (con)</div>
        </div>
        <div class="kpi {'kpi-ok' if seq_hub.get('events',0) >= total_expected else 'kpi-warn'}">
          <div class="kpi-value">{seq_hub.get('events', 0)}/{total_expected}</div>
          <div class="kpi-label">Hub events (seq)</div>
        </div>
        <div class="kpi {'kpi-ok' if con_hub.get('events',0) >= total_expected else 'kpi-warn'}">
          <div class="kpi-value">{con_hub.get('events', 0)}/{total_expected}</div>
          <div class="kpi-label">Hub events (con)</div>
        </div>
      </div>
    </div>
  </div>
</div>

<div class="section">
  <h2>Benchmark Charts</h2>
  <div class="charts">
    <div class="chart-card wide">
      <h2>Sync Wall Time: Sequential vs Concurrent ({n_clinics} clinics, {profile.get("bandwidth_kbps","?")}kbps)</h2>
      <canvas id="wallTimeChart" style="max-height:220px"></canvas>
    </div>
    <div class="chart-card">
      <h2>Per-Clinic Sync Duration (seconds)</h2>
      <canvas id="perClinicChart"></canvas>
    </div>
    <div class="chart-card">
      <h2>Bytes Transferred vs IPFS Estimate</h2>
      <canvas id="bytesChart"></canvas>
    </div>
    {quantile_chart}
  </div>
</div>

<div class="section">
  <h2>Hub Prometheus Metrics</h2>
  <div class="grid">
    <div class="kpi">
      <div class="kpi-value">{seq_session_count}</div>
      <div class="kpi-label">Sessions served (seq)</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{con_session_count}</div>
      <div class="kpi-label">Sessions served (con)</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{seq_avg:.2f}s</div>
      <div class="kpi-label">Avg session duration (seq)</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{con_avg:.2f}s</div>
      <div class="kpi-label">Avg session duration (con)</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{seq_bytes/1024:.1f} KB</div>
      <div class="kpi-label">Total bytes (seq)</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{con_bytes/1024:.1f} KB</div>
      <div class="kpi-label">Total bytes (con)</div>
    </div>
  </div>
</div>

{FEATURE_TABLE}

<footer>
  Generated by ZamSync report.py &mdash;
  <a href="https://github.com/Etoile-Bleu/ZamSync" style="color:var(--accent)">github.com/Etoile-Bleu/ZamSync</a>
</footer>

<script>
{JS_COLORS}

new Chart(document.getElementById('wallTimeChart'), {{
  type: 'bar',
  data: {{
    labels: ['Sequential (--max-peers 1)', 'Concurrent (--max-peers 16)'],
    datasets: [{{
      label: 'Sync wall time (s)',
      data: [{seq_wall}, {con_wall}],
      backgroundColor: [C.seq, C.con],
      borderColor: [C.seq_b, C.con_b],
      borderWidth: 1
    }}]
  }},
  options: {{ ...base, indexAxis: 'y',
    plugins: {{ ...base.plugins,
      title: {{ display: true, color: '#94a3b8',
        text: '{speedup}x speedup -- {seq_wall}s sequential vs {con_wall}s concurrent for {n_clinics} clinics' }} }} }}
}});

new Chart(document.getElementById('perClinicChart'), {{
  type: 'bar',
  data: {{
    labels: {json.dumps(clinic_labels)},
    datasets: [
      {{ label: 'Sequential', data: {json.dumps(seq_times)}, backgroundColor: C.seq, borderColor: C.seq_b, borderWidth: 1 }},
      {{ label: 'Concurrent', data: {json.dumps(con_times)}, backgroundColor: C.con, borderColor: C.con_b, borderWidth: 1 }}
    ]
  }},
  options: base
}});

new Chart(document.getElementById('bytesChart'), {{
  type: 'bar',
  data: {{
    labels: ['ZamSync seq', 'ZamSync con', 'IPFS (estimated)'],
    datasets: [{{
      label: 'Total bytes transferred',
      data: [{seq_bytes}, {con_bytes}, {ipfs_bytes}],
      backgroundColor: [C.seq, C.con, C.ipfs],
      borderColor: [C.seq_b, C.con_b, C.ipfs_b],
      borderWidth: 1
    }}]
  }},
  options: base
}});

{quantile_js}
</script>
</body>
</html>
"""
    out = results_dir / "report.html"
    out.write_text(html, encoding="utf-8")
    print(f"Report: {out.resolve()}")


# ---- Single-run fallback report ---------------------------------------------

def make_single_report(results_dir: Path):
    scenario, nodes, prom = load_scenario(results_dir)
    hub = next((n for n in nodes if n.get("role") == "hub"), None)
    clinics = [n for n in nodes if n.get("role") == "clinic"]
    if not nodes:
        print("No metrics found.")
        sys.exit(1)

    profile = scenario.get("profile", {})
    events_per_clinic = scenario.get("events_per_clinic", 500)
    total_expected = events_per_clinic * len(clinics)
    hub_events = hub["events"] if hub else 0
    convergence_pct = (hub_events / total_expected * 100) if total_expected > 0 else 0

    clinic_names = [c["node"] for c in clinics]
    sync_times   = [c.get("sync_duration_s", 0) for c in clinics]
    bytes_sent   = [c.get("bytes_sent", 0) for c in clinics]
    memory_rss   = [c.get("memory_rss_kb", 0) / 1024 for c in clinics]
    ipfs_bytes   = [int(events_per_clinic * IPFS_COMPARISON["bytes_per_event"]) for _ in clinics]

    prom_received = prom_all(prom, "zamsync_sync_events_received_total", "peer")
    prom_session_sum   = prom_get(prom, "zamsync_sync_duration_seconds_sum", role="responder") or 0
    prom_session_count = int(prom_get(prom, "zamsync_sync_duration_seconds_count", role="responder") or 0)
    prom_avg = prom_session_sum / prom_session_count if prom_session_count > 0 else 0

    sync_wall = scenario.get("sync_wall_s", scenario.get("wall_total_s", 0))
    sum_sync  = scenario.get("sum_sync_s", sum(sync_times))
    speedup   = round(sum_sync / sync_wall, 1) if sync_wall > 0 else 1.0
    is_con    = scenario.get("serving_mode") == "concurrent"

    peer_labels = [f"peer-{p[:6]}" for p in prom_received]
    peer_values = [int(v) for v in prom_received.values()]

    run_date = scenario.get("scenario_date", datetime.now(timezone.utc).isoformat())

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ZamSync Hospital Network Simulation Report</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.min.js"></script>
<style>{CSS}</style>
</head>
<body>

<h1>ZamSync Hospital Network Simulation</h1>
<p class="subtitle">
  Profile: {scenario.get("network_profile","bhutan_2g")} &mdash;
  {profile.get("label","")} &mdash;
  {len(clinics)} clinic node(s) &mdash;
  {events_per_clinic} events per clinic &mdash;
  Run: {run_date}
</p>

<div class="section">
  <h2>Simulation Results</h2>
  <div class="grid">
    <div class="kpi {'kpi-ok' if convergence_pct == 100 else 'kpi-warn'}">
      <div class="kpi-value">{convergence_pct:.1f}%</div>
      <div class="kpi-label">Hub Convergence</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{hub_events}</div>
      <div class="kpi-label">Events on Hub / {total_expected} expected</div>
    </div>
    <div class="kpi {'kpi-ok' if is_con else ''}">
      <div class="kpi-value">{'Concurrent' if is_con else 'Sequential'}</div>
      <div class="kpi-label">Serving Mode</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{sync_wall}s</div>
      <div class="kpi-label">Sync Wall Time</div>
    </div>
    <div class="kpi kpi-ok">
      <div class="kpi-value">{speedup}x</div>
      <div class="kpi-label">Concurrent Speedup</div>
    </div>
    <div class="kpi">
      <div class="kpi-value">{prom_avg:.2f}s</div>
      <div class="kpi-label">Avg Hub Session (Prometheus)</div>
    </div>
  </div>
</div>

<div class="section">
  <div class="charts">
    <div class="chart-card">
      <h2>Sync Duration per Clinic (seconds)</h2>
      <canvas id="syncChart"></canvas>
    </div>
    <div class="chart-card">
      <h2>Bytes: ZamSync vs IPFS</h2>
      <canvas id="bwChart"></canvas>
    </div>
    {'<div class="chart-card"><h2>Events Received per Peer (Prometheus)</h2><canvas id="peerChart"></canvas></div>' if prom_received else ''}
    <div class="chart-card">
      <h2>Memory: ZamSync vs IPFS Daemon</h2>
      <canvas id="memChart"></canvas>
    </div>
  </div>
</div>

{FEATURE_TABLE}

<footer>
  Generated by ZamSync report.py &mdash;
  <a href="https://github.com/Etoile-Bleu/ZamSync" style="color:var(--accent)">github.com/Etoile-Bleu/ZamSync</a>
</footer>

<script>
{JS_COLORS}

new Chart(document.getElementById('syncChart'), {{
  type: 'bar',
  data: {{ labels: {json.dumps(clinic_names)},
    datasets: [{{ label: 'Sync duration (s)', data: {json.dumps(sync_times)}, backgroundColor: C.con, borderColor: C.con_b, borderWidth: 1 }}] }},
  options: {{ ...base, plugins: {{ ...base.plugins, title: {{ display: true, color: '#94a3b8',
    text: 'Profile: {profile.get("label","")} -- {profile.get("delay_ms","?")}ms / {profile.get("bandwidth_kbps","?")}kbps' }} }} }}
}});

new Chart(document.getElementById('bwChart'), {{
  type: 'bar',
  data: {{ labels: {json.dumps(clinic_names)},
    datasets: [
      {{ label: 'ZamSync', data: {json.dumps(bytes_sent)}, backgroundColor: C.con, borderColor: C.con_b, borderWidth: 1 }},
      {{ label: 'IPFS (estimated)', data: {json.dumps(ipfs_bytes)}, backgroundColor: C.ipfs, borderColor: C.ipfs_b, borderWidth: 1 }}
    ] }},
  options: base
}});

{'new Chart(document.getElementById("peerChart"), { type: "bar", data: { labels: ' + json.dumps(peer_labels) + ', datasets: [{ label: "Events received (real)", data: ' + json.dumps(peer_values) + ', backgroundColor: C.con, borderColor: C.con_b, borderWidth: 1 }] }, options: base });' if prom_received else ''}

new Chart(document.getElementById('memChart'), {{
  type: 'bar',
  data: {{ labels: {json.dumps(clinic_names + ['IPFS daemon'])},
    datasets: [{{ label: 'RSS (MB)',
      data: {json.dumps([round(m, 1) for m in memory_rss] + [IPFS_COMPARISON['memory_mb']])},
      backgroundColor: {json.dumps(['rgba(99,102,241,0.8)'] * len(clinics) + ['rgba(239,68,68,0.7)'])},
      borderColor:     {json.dumps(['rgba(99,102,241,1)'] * len(clinics) + ['rgba(239,68,68,1)'])},
      borderWidth: 1 }}] }},
  options: base
}});
</script>
</body>
</html>
"""
    out = results_dir / "report.html"
    out.write_text(html, encoding="utf-8")
    print(f"Report: {out.resolve()}")


# ---- Entry point ------------------------------------------------------------

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: report.py <results-dir>")
        sys.exit(1)
    results_dir = Path(sys.argv[1])
    results_dir.mkdir(parents=True, exist_ok=True)

    if (results_dir / "seq").is_dir() and (results_dir / "con").is_dir():
        make_comparison_report(results_dir)
    else:
        make_single_report(results_dir)
