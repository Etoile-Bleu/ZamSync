use crate::metrics::start_metrics_server;
use crate::util::{data_dir, flag_value, load_encryption_key, load_tls_config, node_id_from_dir, EventCounter};
use std::time::{Duration, Instant};
use zamsync_core::NodeId;
use zamsync_network::{TcpTransport, TlsTcpTransport};
use zamsync_storage::{EncryptionKey, SyncSession, ZamEngine};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let peer_addr = args.get(3).ok_or("missing peer-addr")?;
    let peer_id: u32 = args.get(4).ok_or("missing peer-id")?.parse()?;
    let interval_secs: u64 = flag_value(args, "--interval")
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let use_tls = args.contains(&"--tls".to_string());
    let enc_key = load_encryption_key(args)?;

    if let Some(metrics_addr) = flag_value(args, "--metrics") {
        start_metrics_server(metrics_addr)?;
    }

    let node_id = node_id_from_dir(&dir);
    let peer = NodeId(peer_id);
    let interval = Duration::from_secs(interval_secs);

    let mode = if use_tls { "TLS" } else { "TCP" };
    println!(
        "daemon: node {} syncing with peer {} ({}) every {}s",
        node_id.0, peer_id, mode, interval_secs
    );

    loop {
        let tick_start = Instant::now();
        match sync_once(&dir, node_id, peer, peer_addr, use_tls, enc_key.as_ref()) {
            Ok((sent, received)) => {
                if sent > 0 || received > 0 {
                    println!(
                        "[sync] peer={} sent={} received={}",
                        peer_id, sent, received
                    );
                } else {
                    println!("[sync] peer={} already in sync", peer_id);
                }
            }
            Err(e) => eprintln!("[sync] peer={} error: {}", peer_id, e),
        }

        let elapsed = tick_start.elapsed();
        if elapsed < interval {
            std::thread::sleep(interval - elapsed);
        }
    }
}

fn sync_once(
    dir: &std::path::Path,
    node_id: NodeId,
    peer: NodeId,
    peer_addr: &str,
    use_tls: bool,
    enc_key: Option<&EncryptionKey>,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let mut engine = match enc_key {
        Some(key) => ZamEngine::open_wal_encrypted(dir, node_id, EventCounter::default(), key.clone())?,
        None => ZamEngine::open_wal(dir, node_id, EventCounter::default())?,
    };

    let stats = if use_tls {
        let tls_config = load_tls_config(dir)?;
        let mut transport = TlsTcpTransport::bind("0.0.0.0:0", &tls_config)?;
        transport.connect(peer, peer_addr)?;
        SyncSession::new(&mut engine, &mut transport).sync(peer)?
    } else {
        let mut transport = TcpTransport::bind("0.0.0.0:0")?;
        transport.connect(peer, peer_addr)?;
        SyncSession::new(&mut engine, &mut transport).sync(peer)?
    };

    engine.sync()?;
    Ok((stats.events_sent, stats.events_received))
}
