use std::env;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_network::TcpTransport;
use zamsync_storage::{SyncSession, ZamEngine};

#[derive(Default)]
struct EventCounter {
    count: usize,
    last_seq: Option<SequenceNumber>,
}

impl StateStore for EventCounter {
    fn apply_event(&mut self, seq: SequenceNumber, _event: &Event) -> ZamResult<()> {
        self.count += 1;
        self.last_seq = Some(seq);
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        self.last_seq
    }
}

fn usage() {
    eprintln!(
        "Usage:
  zamsync info   <data-dir>
  zamsync submit <data-dir> <payload>
  zamsync sync   <data-dir> <peer-addr> <peer-id>
  zamsync serve  <data-dir> <bind-addr>"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("info") => cmd_info(&args),
        Some("submit") => cmd_submit(&args),
        Some("sync") => cmd_sync(&args),
        Some("serve") => cmd_serve(&args),
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}

fn cmd_info(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let node_id = node_id_from_dir(&dir);
    let engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;

    println!("node_id  : {}", node_id.0);
    println!("data_dir : {}", dir.display());
    println!("events   : {}", engine.state().count);
    let vv = &engine.replication_state().local_vv;
    if vv.entries.is_empty() {
        println!("vv       : (empty)");
    } else {
        for (node, seq) in &vv.entries {
            println!("vv       : node {} @ seq {}", node, seq.0);
        }
    }
    Ok(())
}

fn cmd_submit(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let payload = args.get(3).ok_or("missing payload")?.as_bytes().to_vec();
    let node_id = node_id_from_dir(&dir);
    let mut engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;
    let seq = engine.submit(1, payload)?;
    engine.sync()?;
    println!("submitted seq={}", seq.0);
    Ok(())
}

fn cmd_sync(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let peer_addr = args.get(3).ok_or("missing peer-addr")?;
    let peer_id: u32 = args.get(4).ok_or("missing peer-id")?.parse()?;

    let node_id = node_id_from_dir(&dir);
    let mut engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;
    let peer = NodeId(peer_id);

    const MAX_ATTEMPTS: u32 = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        let mut transport = TcpTransport::bind("0.0.0.0:0")?;
        let connect_result = transport.connect(peer, peer_addr);
        let sync_result = connect_result
            .and_then(|()| SyncSession::new(&mut engine, &mut transport).sync(peer));

        match sync_result {
            Ok(stats) => {
                println!(
                    "sync done: sent={} received={}",
                    stats.events_sent, stats.events_received
                );
                return Ok(());
            }
            Err(ref e) if is_transient(e) && attempt < MAX_ATTEMPTS => {
                let delay_ms = 100u64 * (1 << (attempt - 1));
                eprintln!(
                    "sync attempt {}/{MAX_ATTEMPTS} failed ({}), retrying in {delay_ms}ms",
                    attempt, e
                );
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

fn cmd_serve(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let bind_addr = args.get(3).ok_or("missing bind-addr")?;

    let node_id = node_id_from_dir(&dir);
    let mut transport = TcpTransport::bind(bind_addr)?;
    println!(
        "node {} listening on {}",
        node_id.0,
        transport.local_addr()?
    );

    loop {
        let mut engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;

        let peer_id = match transport.accept_any() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("accept error: {e}");
                continue;
            }
        };
        println!("peer {} connected", peer_id.0);

        match SyncSession::new(&mut engine, &mut transport).serve_one(peer_id) {
            Ok(stats) => println!(
                "sync with peer {} done: sent={} received={}",
                peer_id.0, stats.events_sent, stats.events_received
            ),
            Err(e) => eprintln!("sync with peer {} failed: {e}", peer_id.0),
        }
        transport.disconnect(peer_id);
    }
}

fn data_dir(args: &[String], pos: usize) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = PathBuf::from(args.get(pos).ok_or("missing data-dir")?);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn node_id_from_dir(dir: &std::path::Path) -> NodeId {
    let id_file = dir.join(".node_id");
    if let Ok(bytes) = std::fs::read(&id_file) {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            if let Ok(n) = s.trim().parse::<u32>() {
                return NodeId(n);
            }
        }
    }
    let id = rand_u32();
    let _ = std::fs::write(&id_file, id.to_string());
    NodeId(id)
}

fn is_transient(e: &zamsync_core::ZamError) -> bool {
    match e {
        zamsync_core::ZamError::Io(io_err) => matches!(
            io_err.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionRefused
        ),
        _ => false,
    }
}

fn rand_u32() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;
    let mut h = DefaultHasher::new();
    SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    h.finish() as u32
}
