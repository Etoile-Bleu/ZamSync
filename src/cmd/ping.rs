use std::path::Path;
use std::time::{Duration, Instant};

use zamsync_core::ports::Transport;
use zamsync_core::{NodeId, SyncMessage, VersionVector};
use zamsync_network::{TcpTransport, TlsTcpTransport};

use crate::color;
use crate::util::{data_dir, flag_value, load_tls_config, node_id_from_dir};

const DEFAULT_COUNT: usize = 3;
const DEFAULT_TIMEOUT_MS: u64 = 5_000;

// Placeholder key used while the remote node's real ID is unknown.
// The actual peer NodeId is extracted from the Handshake message payload.
const PROBE_PEER: NodeId = NodeId(0);

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let peer_addr = args
        .get(3)
        .ok_or("usage: zamsync ping <data-dir> <peer-addr> [--tls] [--count N] [--timeout MS]")?;
    let use_tls = args.contains(&"--tls".to_string());
    let count: usize = flag_value(args, "--count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_COUNT);
    let timeout = Duration::from_millis(
        flag_value(args, "--timeout")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_MS),
    );

    let local_id = node_id_from_dir(&dir);
    let tls_tag = if use_tls {
        format!("  {}", color::green("[TLS]"))
    } else {
        String::new()
    };
    println!(
        "{}  local-node={}{}",
        color::bold(&format!("PING {peer_addr}")),
        local_id.0,
        tls_tag,
    );

    let mut rtt_samples: Vec<Duration> = Vec::with_capacity(count);
    let mut failures = 0usize;

    for seq in 1..=count {
        match probe(&dir, local_id, peer_addr, use_tls, timeout) {
            Ok((peer_id, rtt)) => {
                let rtt_ms = rtt.as_millis();
                let tls_ok = if use_tls {
                    format!("  {}", color::green("tls=ok"))
                } else {
                    String::new()
                };
                println!(
                    "  {}  peer={}  rtt={}{}",
                    color::dim(&format!("seq={seq}")),
                    peer_id.0,
                    color::rtt(rtt_ms),
                    tls_ok,
                );
                rtt_samples.push(rtt);
            }
            Err(e) => {
                println!(
                    "  {}  {}",
                    color::dim(&format!("seq={seq}")),
                    color::red(&format!("error: {e}")),
                );
                failures += 1;
            }
        }
    }

    println!("{}", color::dim("---"));
    let successes = count - failures;
    println!("{successes}/{count}  {}", color::loss(failures, count));
    if !rtt_samples.is_empty() {
        let min = rtt_samples.iter().min().unwrap().as_millis();
        let max = rtt_samples.iter().max().unwrap().as_millis();
        let avg =
            rtt_samples.iter().map(|d| d.as_millis()).sum::<u128>() / rtt_samples.len() as u128;
        println!(
            "rtt  min={}  avg={}  max={}",
            color::rtt(min),
            color::rtt(avg),
            color::rtt(max),
        );
    }

    if failures == count {
        std::process::exit(1);
    }
    Ok(())
}

/// Establishes one probe connection, exchanges a Handshake, and returns the
/// remote node's ID with the round-trip time (connection setup included).
fn probe(
    dir: &Path,
    local_id: NodeId,
    peer_addr: &str,
    use_tls: bool,
    timeout: Duration,
) -> Result<(NodeId, Duration), Box<dyn std::error::Error>> {
    let t0 = Instant::now();
    if use_tls {
        let tls_config = load_tls_config(dir)?;
        let mut transport = TlsTcpTransport::bind("0.0.0.0:0", &tls_config)?;
        transport.connect(PROBE_PEER, peer_addr)?;
        send_handshake(&mut transport, local_id)?;
        recv_handshake(&mut transport, t0, timeout)
    } else {
        let mut transport = TcpTransport::bind("0.0.0.0:0")?;
        transport.connect(PROBE_PEER, peer_addr)?;
        send_handshake(&mut transport, local_id)?;
        recv_handshake(&mut transport, t0, timeout)
    }
}

fn send_handshake<T: Transport>(
    transport: &mut T,
    local_id: NodeId,
) -> Result<(), Box<dyn std::error::Error>> {
    transport.send(
        PROBE_PEER,
        &SyncMessage::Handshake {
            node_id: local_id,
            vv: VersionVector::new(),
        },
    )?;
    Ok(())
}

/// Polls until the peer's Handshake arrives or the deadline expires.
/// The NodeId is read from the message payload, not the connection key.
fn recv_handshake<T: Transport>(
    transport: &mut T,
    t0: Instant,
    timeout: Duration,
) -> Result<(NodeId, Duration), Box<dyn std::error::Error>> {
    loop {
        if t0.elapsed() > timeout {
            return Err(format!("timeout after {}ms", timeout.as_millis()).into());
        }
        match transport.receive()? {
            Some((_, SyncMessage::Handshake { node_id, .. })) => {
                return Ok((node_id, t0.elapsed()));
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}
