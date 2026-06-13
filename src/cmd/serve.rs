use crate::metrics::start_metrics_server;
use crate::util::{data_dir, flag_value, load_encryption_key, load_tls_config, node_id_from_dir, EventCounter};
use std::path::Path;
use zamsync_core::NodeId;
use zamsync_network::{TcpTransport, TlsTcpTransport};
use zamsync_storage::{EncryptionKey, FilePeerStore, SyncSession, WalEventStore, ZamEngine};

type Engine = ZamEngine<WalEventStore, FilePeerStore, EventCounter>;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let bind_addr = args.get(3).ok_or("missing bind-addr")?;
    let use_tls = args.contains(&"--tls".to_string());
    let enc_key = load_encryption_key(args)?;

    if let Some(metrics_addr) = flag_value(args, "--metrics") {
        start_metrics_server(metrics_addr)?;
    }

    let node_id = node_id_from_dir(&dir);

    if use_tls {
        let tls_config = load_tls_config(&dir)?;
        let mut transport = TlsTcpTransport::bind(bind_addr, &tls_config)?;
        println!("node {} TLS listening on {}", node_id.0, transport.local_addr()?);
        tls_loop(node_id, &dir, enc_key, &mut transport);
    } else {
        let mut transport = TcpTransport::bind(bind_addr)?;
        println!("node {} listening on {}", node_id.0, transport.local_addr()?);
        tcp_loop(node_id, &dir, enc_key, &mut transport);
    }
    Ok(())
}

fn open_engine(dir: &Path, node_id: NodeId, enc_key: &Option<EncryptionKey>) -> zamsync_core::ZamResult<Engine> {
    match enc_key {
        Some(key) => ZamEngine::open_wal_encrypted(dir, node_id, EventCounter::default(), key.clone()),
        None => ZamEngine::open_wal(dir, node_id, EventCounter::default()),
    }
}

fn tcp_loop(node_id: NodeId, dir: &Path, enc_key: Option<EncryptionKey>, transport: &mut TcpTransport) {
    loop {
        let mut engine = match open_engine(dir, node_id, &enc_key) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("engine open error: {e}");
                continue;
            }
        };
        let peer_id = match transport.accept_any() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("accept error: {e}");
                continue;
            }
        };
        println!("peer {} connected", peer_id.0);
        match SyncSession::new(&mut engine, transport).serve_one(peer_id) {
            Ok(stats) => println!(
                "sync with peer {} done: sent={} received={}",
                peer_id.0, stats.events_sent, stats.events_received
            ),
            Err(e) => eprintln!("sync with peer {} failed: {e}", peer_id.0),
        }
        transport.disconnect(peer_id);
    }
}

fn tls_loop(node_id: NodeId, dir: &Path, enc_key: Option<EncryptionKey>, transport: &mut TlsTcpTransport) {
    loop {
        let mut engine = match open_engine(dir, node_id, &enc_key) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("engine open error: {e}");
                continue;
            }
        };
        let peer_id = match transport.accept_any() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("TLS accept error: {e}");
                continue;
            }
        };
        println!("TLS peer {} connected", peer_id.0);
        match SyncSession::new(&mut engine, transport).serve_one(peer_id) {
            Ok(stats) => println!(
                "TLS sync with peer {} done: sent={} received={}",
                peer_id.0, stats.events_sent, stats.events_received
            ),
            Err(e) => eprintln!("TLS sync with peer {} failed: {e}", peer_id.0),
        }
        transport.disconnect(peer_id);
    }
}
