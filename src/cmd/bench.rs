use crate::util::{data_dir, flag_value, node_id_from_dir, EventCounter};
use std::time::Instant;
use zamsync_storage::ZamEngine;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let n_events: usize = flag_value(args, "--events")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);

    // ~64-byte payload: representative of a compact domain event header.
    let payload = b"bench-payload-zamsync-0123456789abcdef0123456789abcdef01234567".to_vec();

    println!(
        "bench: {} events, payload {} bytes",
        n_events,
        payload.len()
    );
    println!("data : {}", dir.display());

    // --- submit ---
    let node_id = node_id_from_dir(&dir);
    let mut engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;

    let t0 = Instant::now();
    for _ in 0..n_events {
        engine.submit(1, payload.clone())?;
    }
    engine.sync()?;
    let submit_secs = t0.elapsed().as_secs_f64();

    // --- reload (simulates startup cost) ---
    let t1 = Instant::now();
    let engine2 = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;
    let reload_secs = t1.elapsed().as_secs_f64();
    let rss_after_reload = rss_kb();

    // --- wal size ---
    let wal_bytes = std::fs::metadata(dir.join("events.wal"))
        .map(|m| m.len())
        .unwrap_or(0);

    let _ = engine2;

    // --- report ---
    println!();
    println!("=== submit ===");
    println!("  time       : {:.3}s", submit_secs);
    println!(
        "  throughput : {:.0} events/sec",
        n_events as f64 / submit_secs
    );
    println!("  wal size   : {} KB", wal_bytes / 1024);

    println!();
    println!("=== reload (WAL replay) ===");
    println!("  time       : {:.3}s", reload_secs);

    println!();
    println!("=== memory (after reload) ===");
    match rss_after_reload {
        Some(kb) => {
            let mb = kb / 1024;
            let target = if mb < 100 { "OK" } else { "OVER TARGET" };
            println!(
                "  rss        : {} KB ({} MB)  [target: <100 MB] -- {}",
                kb, mb, target
            );
        }
        None => println!("  rss        : (not available on this platform)"),
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn rss_kb() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut pmc = std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            Some(pmc.WorkingSetSize as u64 / 1024)
        } else {
            None
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn rss_kb() -> Option<u64> {
    None
}
