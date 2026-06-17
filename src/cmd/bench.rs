use crate::util::{data_dir, flag_value, node_id_from_dir, EventCounter};
use std::thread;
use std::time::Instant;
use zamsync_core::NodeId;
use zamsync_network::TcpTransport;
use zamsync_storage::{SyncSession, ZamEngine};

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

    // --- sync bandwidth benchmark ---
    println!();
    println!("=== sync bandwidth ===");
    bench_sync_bandwidth(n_events, &payload)?;

    Ok(())
}

/// Measures sync throughput and wire bandwidth by syncing two in-process nodes
/// over loopback TCP. Reports events/sec and MB/s for:
///   - uncapped full sync
///   - capped sync at 10% budget (simulates VSAT session limits)
fn bench_sync_bandwidth(n_events: usize, payload: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let hub_dir = tempfile::tempdir()?;
    let clinic_dir = tempfile::tempdir()?;
    let hub_id = NodeId(1000);
    let clinic_id = NodeId(1001);

    // Populate hub with n_events
    {
        let mut eng = ZamEngine::open_wal(hub_dir.path(), hub_id, EventCounter::default())?;
        for _ in 0..n_events {
            eng.submit(1, payload.to_vec())?;
        }
        eng.sync()?;
    }
    ZamEngine::open_wal(clinic_dir.path(), clinic_id, EventCounter::default())?.sync()?;

    // ---- Uncapped full sync ------------------------------------------------
    let hub_path = hub_dir.path().to_path_buf();
    let mut hub_transport = TcpTransport::bind("127.0.0.1:0")?;
    let hub_addr = hub_transport.local_addr()?.to_string();
    let hub_thread = thread::spawn(move || {
        let mut pt = hub_transport.accept_split().unwrap();
        let peer_id = pt.peer_id();
        let mut eng = ZamEngine::open_wal(&hub_path, hub_id, EventCounter::default()).unwrap();
        SyncSession::new(&mut eng, &mut pt)
            .serve_one(peer_id)
            .unwrap();
    });

    let mut clinic_transport = TcpTransport::bind("127.0.0.1:0")?;
    let mut clinic_eng =
        ZamEngine::open_wal(clinic_dir.path(), clinic_id, EventCounter::default())?;
    clinic_transport.connect(hub_id, &hub_addr)?;
    let t_sync = Instant::now();
    let stats = SyncSession::new(&mut clinic_eng, &mut clinic_transport).sync(hub_id)?;
    let sync_secs = t_sync.elapsed().as_secs_f64();
    hub_thread.join().unwrap();
    clinic_eng.sync()?;

    let mb_sent = stats.bytes_sent as f64 / (1024.0 * 1024.0);
    let events_per_sec = if sync_secs > 0.0 {
        stats.events_received as f64 / sync_secs
    } else {
        f64::INFINITY
    };
    let mb_per_sec = if sync_secs > 0.0 {
        mb_sent / sync_secs
    } else {
        f64::INFINITY
    };

    println!("  [full sync -- no cap]");
    println!("    events received : {}", stats.events_received);
    println!(
        "    wire bytes sent : {:.2} MB ({} B)",
        mb_sent, stats.bytes_sent
    );
    println!(
        "    compression     : {:.1}x (vs raw payload)",
        (n_events * payload.len()) as f64 / stats.bytes_sent.max(1) as f64
    );
    println!("    time            : {:.3}s", sync_secs);
    println!("    throughput      : {:.0} events/sec", events_per_sec);
    println!("    bandwidth       : {:.2} MB/s", mb_per_sec);

    // ---- Capped sync (10% of full wire bytes) ------------------------------
    // Reset the clinic WAL so we can measure the capped path from scratch.
    let clinic_dir2 = tempfile::tempdir()?;
    ZamEngine::open_wal(clinic_dir2.path(), clinic_id, EventCounter::default())?.sync()?;

    let hub_path2 = hub_dir.path().to_path_buf();
    let mut hub_transport2 = TcpTransport::bind("127.0.0.1:0")?;
    let hub_addr2 = hub_transport2.local_addr()?.to_string();
    let hub_thread2 = thread::spawn(move || {
        let mut pt = hub_transport2.accept_split().unwrap();
        let peer_id = pt.peer_id();
        let mut eng = ZamEngine::open_wal(&hub_path2, hub_id, EventCounter::default()).unwrap();
        SyncSession::new(&mut eng, &mut pt)
            .serve_one(peer_id)
            .unwrap();
    });

    let cap = (stats.bytes_sent / 10).max(1);
    let mut clinic_transport2 = TcpTransport::bind("127.0.0.1:0")?;
    let mut clinic_eng2 =
        ZamEngine::open_wal(clinic_dir2.path(), clinic_id, EventCounter::default())?;
    clinic_transport2.connect(hub_id, &hub_addr2)?;
    let t_capped = Instant::now();
    let stats_capped = SyncSession::new(&mut clinic_eng2, &mut clinic_transport2)
        .with_max_bytes(cap)
        .sync(hub_id)?;
    let capped_secs = t_capped.elapsed().as_secs_f64();
    hub_thread2.join().unwrap();

    println!();
    println!("  [capped sync -- budget {} B (~10% of full)]", cap);
    println!("    events received : {}", stats_capped.events_received);
    println!("    wire bytes sent : {} B", stats_capped.bytes_sent);
    println!("    budget exhausted: {}", stats_capped.budget_exhausted);
    println!("    time            : {:.3}s", capped_secs);
    if stats_capped.budget_exhausted {
        let pct = stats_capped.events_received as f64 / n_events as f64 * 100.0;
        println!(
            "    progress        : {:.1}% events delivered in this window",
            pct
        );
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
