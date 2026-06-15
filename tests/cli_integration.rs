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
    assert!(path.exists(), "zamsync binary not found at {}", path.display());
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

    let out = Command::new(&bin)
        .args(["info", dir_s])
        .output()
        .unwrap();
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
