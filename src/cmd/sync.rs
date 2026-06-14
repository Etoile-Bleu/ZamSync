use crate::metrics::start_metrics_server;
use crate::util::{
    data_dir, flag_value, is_transient, load_encryption_key, load_schema, load_tls_config,
    node_id_from_dir, open_engine,
};
use zamsync_core::NodeId;
use zamsync_network::{TcpTransport, TlsTcpTransport};
use zamsync_storage::SyncSession;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let peer_addr = args.get(3).ok_or("missing peer-addr")?;
    let peer_id: u32 = args.get(4).ok_or("missing peer-id")?.parse()?;
    let use_tls = args.contains(&"--tls".to_string());
    let enc_key = load_encryption_key(args)?;
    let schema = load_schema(args)?;

    if let Some(metrics_addr) = flag_value(args, "--metrics") {
        start_metrics_server(metrics_addr)?;
    }

    let node_id = node_id_from_dir(&dir);
    let mut engine = open_engine(&dir, node_id, enc_key, schema)?;
    let peer = NodeId(peer_id);

    const MAX_ATTEMPTS: u32 = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        let sync_result = if use_tls {
            let tls_config = load_tls_config(&dir)?;
            let mut transport = TlsTcpTransport::bind("0.0.0.0:0", &tls_config)?;
            transport
                .connect(peer, peer_addr)
                .and_then(|()| SyncSession::new(&mut engine, &mut transport).sync(peer))
        } else {
            let mut transport = TcpTransport::bind("0.0.0.0:0")?;
            transport
                .connect(peer, peer_addr)
                .and_then(|()| SyncSession::new(&mut engine, &mut transport).sync(peer))
        };

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
