// CLI integration tests -- run with: cargo test --features integration --test cli_integration
//
// Each test executes the real `zamsync` binary against a temporary directory.
// The binary path is provided by Cargo via CARGO_BIN_EXE_zamsync.

#![cfg(feature = "integration")]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

fn bin() -> std::path::PathBuf {
    // CARGO_BIN_EXE_zamsync is set by Cargo for integration tests in the same package.
    let path = std::env::var("CARGO_BIN_EXE_zamsync")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = std::env::current_exe().unwrap();
            p.pop();
            p.pop(); // deps/ -> debug/
            p.push("zamsync");
            if cfg!(windows) {
                p.set_extension("exe");
            }
            p
        });
    assert!(
        path.exists(),
        "zamsync binary not found at {}",
        path.display()
    );
    path
}

fn read_node_id(dir: &Path) -> u32 {
    std::fs::read_to_string(dir.join(".node_id"))
        .expect(".node_id missing")
        .trim()
        .parse()
        .expect("invalid node_id")
}

// ---- info --------------------------------------------------------------------

#[test]
fn test_info_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .args(["info", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "info exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("events   : 0"),
        "expected 0 events in fresh dir: {stdout}"
    );
}

#[test]
fn test_info_peer_breakdown() {
    let hub_dir = tempfile::tempdir().unwrap();
    let clinic_a_dir = tempfile::tempdir().unwrap();
    let clinic_b_dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let hub_dir_s = hub_dir.path().to_str().unwrap();
    let clinic_a_s = clinic_a_dir.path().to_str().unwrap();
    let clinic_b_s = clinic_b_dir.path().to_str().unwrap();

    // Submit events from clinic A
    for i in 0..3u32 {
        let out = Command::new(&bin)
            .args(["submit", clinic_a_s, &format!("a-event-{i}")])
            .output()
            .unwrap();
        assert!(out.status.success(), "submit A failed");
    }

    // Submit events from clinic B
    for i in 0..2u32 {
        let out = Command::new(&bin)
            .args(["submit", clinic_b_s, &format!("b-event-{i}")])
            .output()
            .unwrap();
        assert!(out.status.success(), "submit B failed");
    }

    // Touch hub dir to create .node_id
    Command::new(&bin)
        .args(["info", hub_dir_s])
        .output()
        .unwrap();
    let hub_id = read_node_id(hub_dir.path());
    let clinic_a_id = read_node_id(clinic_a_dir.path());
    let clinic_b_id = read_node_id(clinic_b_dir.path());

    // Start hub
    let mut hub = Command::new(&bin)
        .args(["serve", hub_dir_s, "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let hub_stdout = hub.stdout.take().unwrap();
    let mut reader = BufReader::new(hub_stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let bind_addr = line
        .split("listening on ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("could not parse bind addr: {line:?}"))
        .to_string();

    // Sync clinic A -> hub
    let out = Command::new(&bin)
        .args(["sync", clinic_a_s, &bind_addr, &hub_id.to_string()])
        .output()
        .unwrap();
    assert!(out.status.success(), "sync A failed");

    // Sync clinic B -> hub
    let out = Command::new(&bin)
        .args(["sync", clinic_b_s, &bind_addr, &hub_id.to_string()])
        .output()
        .unwrap();
    assert!(out.status.success(), "sync B failed");

    hub.kill().ok();
    hub.wait().ok();

    // Run info on the hub and check peer breakdown
    let info = Command::new(&bin)
        .args(["info", hub_dir_s])
        .output()
        .unwrap();
    assert!(info.status.success());
    let stdout = String::from_utf8_lossy(&info.stdout);

    // Total should be 5
    assert!(
        stdout.contains("events   : 5"),
        "hub should hold 5 events: {stdout}"
    );

    // Both clinic node IDs must appear in peers section
    assert!(
        stdout.contains(&format!("node {}", clinic_a_id)) && stdout.contains("3 events"),
        "clinic A ({clinic_a_id}) should show 3 events in peers: {stdout}"
    );
    assert!(
        stdout.contains(&format!("node {}", clinic_b_id)) && stdout.contains("2 events"),
        "clinic B ({clinic_b_id}) should show 2 events in peers: {stdout}"
    );

    // "peers:" header must appear
    assert!(
        stdout.contains("peers:"),
        "peers section header missing: {stdout}"
    );
}

// ---- submit ------------------------------------------------------------------

#[test]
fn test_submit_increments_event_count() {
    let dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let dir_s = dir.path().to_str().unwrap();

    for i in 0..3u32 {
        let out = Command::new(&bin)
            .args(["submit", dir_s, &format!("payload-{i}")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "submit {i} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(&format!("submitted seq={i}")),
            "unexpected submit output: {stdout}"
        );
    }

    let out = Command::new(&bin).args(["info", dir_s]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("events   : 3"),
        "info after 3 submits should report 3 events: {stdout}"
    );
}

// ---- compact -----------------------------------------------------------------

#[test]
fn test_compact_after_submit() {
    let dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let dir_s = dir.path().to_str().unwrap();

    Command::new(&bin)
        .args(["submit", dir_s, "data"])
        .output()
        .unwrap();

    let out = Command::new(&bin)
        .args(["compact", dir_s])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "compact failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---- project -----------------------------------------------------------------

#[test]
fn test_project_creates_db_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let dir_s = dir.path().to_str().unwrap();

    // Submit 3 events
    for i in 0..3u32 {
        Command::new(&bin)
            .args(["submit", dir_s, &format!("record-{i}")])
            .output()
            .unwrap();
    }

    // First project run: should insert all 3
    let out = Command::new(&bin)
        .args(["project", dir_s])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "project failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("3 projected"),
        "expected '3 projected' in output: {stdout}"
    );
    assert!(
        stdout.contains("0 already present"),
        "expected '0 already present' in output: {stdout}"
    );

    // projection.db must exist
    assert!(
        dir.path().join("projection.db").exists(),
        "projection.db not created"
    );

    // Second run: all 3 already present, 0 newly projected
    let out2 = Command::new(&bin)
        .args(["project", dir_s])
        .output()
        .unwrap();
    assert!(out2.status.success());
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        stdout2.contains("0 projected") && stdout2.contains("3 already present"),
        "second run should skip all: {stdout2}"
    );
}

#[test]
fn test_project_dry_run_no_file_created() {
    let dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let dir_s = dir.path().to_str().unwrap();

    Command::new(&bin)
        .args(["submit", dir_s, "event-1"])
        .output()
        .unwrap();

    let out = Command::new(&bin)
        .args(["project", dir_s, "--dry-run"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "project --dry-run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("would be projected"),
        "dry-run should mention 'would be projected': {stdout}"
    );

    // Must NOT create the database file
    assert!(
        !dir.path().join("projection.db").exists(),
        "dry-run must not create projection.db"
    );
}

// ---- project (PostgreSQL) ----------------------------------------------------

#[test]
#[ignore]
fn test_project_pg_is_idempotent() {
    let pg_url = match std::env::var("TEST_PG_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("TEST_PG_URL not set – skipping PostgreSQL projection test");
            return;
        }
    };

    let dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let dir_s = dir.path().to_str().unwrap();

    // Submit 3 events
    for i in 0..3u32 {
        Command::new(&bin)
            .args(["submit", dir_s, &format!("pg-record-{i}")])
            .output()
            .unwrap();
    }

    // First project run: should insert all 3
    let out = Command::new(&bin)
        .args(["project", dir_s, "--target", &pg_url])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "project --target postgres failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("3 projected"),
        "expected '3 projected' in output: {stdout}"
    );
    assert!(
        stdout.contains("0 already present"),
        "expected '0 already present' in output: {stdout}"
    );

    // Second run: all 3 already present, 0 newly projected
    let out2 = Command::new(&bin)
        .args(["project", dir_s, "--target", &pg_url])
        .output()
        .unwrap();
    assert!(out2.status.success());
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        stdout2.contains("0 projected") && stdout2.contains("3 already present"),
        "second run should skip all: {stdout2}"
    );
}

// ---- serve + sync ------------------------------------------------------------

/// Start hub on port 0 (OS picks the port), parse the actual address from
/// stdout, submit 5 events from a clinic, sync, verify hub convergence.
#[test]
fn test_serve_and_sync_converges() {
    let hub_dir = tempfile::tempdir().unwrap();
    let clinic_dir = tempfile::tempdir().unwrap();
    let bin = bin();
    let hub_dir_s = hub_dir.path().to_str().unwrap();
    let clinic_dir_s = clinic_dir.path().to_str().unwrap();

    // Submit 5 events from the clinic before syncing
    for i in 0..5u32 {
        Command::new(&bin)
            .args(["submit", clinic_dir_s, &format!("clinic-event-{i}")])
            .output()
            .unwrap();
    }

    // Touch hub dir so .node_id is created before serve reads it
    Command::new(&bin)
        .args(["info", hub_dir_s])
        .output()
        .unwrap();
    let hub_id = read_node_id(hub_dir.path());

    // Start hub on a dynamically-assigned port (":0")
    let mut hub = Command::new(&bin)
        .args(["serve", hub_dir_s, "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // Read the first stdout line to discover the actual bound address.
    // The hub prints: "node <id> listening on <addr> [policy=All] [max-peers=16]"
    let hub_stdout = hub.stdout.take().unwrap();
    let mut reader = BufReader::new(hub_stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let bind_addr = line
        .split("listening on ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("could not parse bind addr from hub output: {line:?}"))
        .to_string();

    // Sync clinic -> hub
    let out = Command::new(&bin)
        .args(["sync", clinic_dir_s, &bind_addr, &hub_id.to_string()])
        .output()
        .unwrap();

    hub.kill().ok();
    hub.wait().ok();

    assert!(
        out.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Verify hub WAL has the 5 clinic events
    let info = Command::new(&bin)
        .args(["info", hub_dir_s])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(
        stdout.contains("events   : 5"),
        "hub should hold 5 events after sync: {stdout}"
    );
}
