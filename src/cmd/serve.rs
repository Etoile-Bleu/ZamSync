use crate::metrics::start_metrics_server;
use crate::util::{
    data_dir, flag_value, load_encryption_key, load_policy, load_schema, load_tls_config,
    node_id_from_dir, EventCounter,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use zamsync_core::NodeId;
use zamsync_network::{TcpPeerTransport, TcpTransport, TlsPeerTransport, TlsTcpTransport};
use zamsync_storage::{
    AccessPolicy, EncryptionKey, FilePeerStore, PayloadSchema, SyncSession, WalEventStore,
    ZamEngine,
};

type Engine = ZamEngine<WalEventStore, FilePeerStore, EventCounter>;

// ---- Connection-rate semaphore -----------------------------------------------

/// Counting semaphore that caps the number of concurrent peer sessions.
/// Uses only `std` -- no external dependency.
struct Semaphore {
    count: Mutex<usize>,
    cvar: Condvar,
    max: usize,
}

/// RAII permit: decrements the count and wakes a waiter when dropped.
struct SemaphorePermit(Arc<Semaphore>);

impl Semaphore {
    fn new(max: usize) -> Arc<Self> {
        Arc::new(Self {
            count: Mutex::new(0),
            cvar: Condvar::new(),
            max,
        })
    }

    /// Block until a slot is free, then claim it.
    fn acquire(self: &Arc<Self>) -> SemaphorePermit {
        let mut count = self.count.lock().unwrap();
        while *count >= self.max {
            count = self.cvar.wait(count).unwrap();
        }
        *count += 1;
        SemaphorePermit(Arc::clone(self))
    }
}

impl Drop for SemaphorePermit {
    fn drop(&mut self) {
        let mut count = self.0.count.lock().unwrap();
        *count -= 1;
        self.0.cvar.notify_one();
    }
}

// ---- Entry point -------------------------------------------------------------

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let bind_addr = args.get(3).ok_or("missing bind-addr")?;
    let use_tls = args.contains(&"--tls".to_string());
    let enc_key = load_encryption_key(args)?;
    let schema = load_schema(args)?;
    let policy = load_policy(args)?;
    let max_peers: usize = flag_value(args, "--max-peers")
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);

    if let Some(metrics_addr) = flag_value(args, "--metrics") {
        start_metrics_server(metrics_addr)?;
    }

    let node_id = node_id_from_dir(&dir);

    if use_tls {
        let tls_config = load_tls_config(&dir)?;
        let mut transport = TlsTcpTransport::bind(bind_addr, &tls_config)?;
        println!(
            "node {} TLS listening on {} [policy={:?}] [max-peers={}]",
            node_id.0,
            transport.local_addr()?,
            policy,
            max_peers,
        );
        tls_loop(node_id, &dir, enc_key, schema, policy, max_peers, &mut transport);
    } else {
        let mut transport = TcpTransport::bind(bind_addr)?;
        println!(
            "node {} listening on {} [policy={:?}] [max-peers={}]",
            node_id.0,
            transport.local_addr()?,
            policy,
            max_peers,
        );
        tcp_loop(node_id, &dir, enc_key, schema, policy, max_peers, &mut transport);
    }
    Ok(())
}

// ---- Shared engine factory ---------------------------------------------------

fn open_for_serve(
    dir: &Path,
    node_id: NodeId,
    enc_key: &Option<EncryptionKey>,
    schema: &PayloadSchema,
    policy: &AccessPolicy,
) -> zamsync_core::ZamResult<Engine> {
    let engine = match enc_key {
        Some(key) => {
            ZamEngine::open_wal_encrypted(dir, node_id, EventCounter::default(), key.clone())?
        }
        None => ZamEngine::open_wal(dir, node_id, EventCounter::default())?,
    };
    Ok(engine
        .with_schema(schema.clone())
        .with_policy(policy.clone()))
}

// ---- Concurrent TCP serve loop -----------------------------------------------
//
// Design: the main thread accepts connections as fast as possible. Each
// accepted peer is handed off to a dedicated worker thread via
// `TcpPeerTransport` (which is `Send`). A counting semaphore caps the number
// of concurrent worker threads at `max_peers` -- when at capacity the main
// thread blocks on `sem.acquire()` after the accept, so the next connection
// is already established but waits for a slot before engine work begins.
//
// The WAL is opened fresh per session inside the worker thread: each session
// gets its own `ZamEngine` instance, avoiding any shared mutable state.

fn serve_peer_tcp(
    dir: PathBuf,
    node_id: NodeId,
    enc_key: Option<EncryptionKey>,
    schema: PayloadSchema,
    policy: AccessPolicy,
    permit: SemaphorePermit,
    mut pt: TcpPeerTransport,
) {
    let _permit = permit; // released when this function returns
    let peer_id = pt.peer_id();
    let mut engine = match open_for_serve(&dir, node_id, &enc_key, &schema, &policy) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("peer {}: engine open error: {e}", peer_id.0);
            return;
        }
    };
    match SyncSession::new(&mut engine, &mut pt).serve_one(peer_id) {
        Ok(stats) => println!(
            "peer {} done: sent={} received={}",
            peer_id.0, stats.events_sent, stats.events_received
        ),
        Err(e) => eprintln!("peer {} sync error: {e}", peer_id.0),
    }
}

fn tcp_loop(
    node_id: NodeId,
    dir: &Path,
    enc_key: Option<EncryptionKey>,
    schema: PayloadSchema,
    policy: AccessPolicy,
    max_peers: usize,
    transport: &mut TcpTransport,
) {
    let sem = Semaphore::new(max_peers);
    loop {
        let pt = match transport.accept_split() {
            Ok(pt) => pt,
            Err(e) => {
                eprintln!("accept error: {e}");
                continue;
            }
        };

        // Block here if already at max_peers -- the accepted connection waits.
        let permit = sem.acquire();

        let dir = dir.to_path_buf();
        let enc_key = enc_key.clone();
        let schema = schema.clone();
        let policy = policy.clone();
        let peer_id = pt.peer_id();

        if let Err(e) = std::thread::Builder::new()
            .name(format!("sync-peer-{}", peer_id.0))
            .stack_size(512 * 1024) // 512 KB -- sync sessions are I/O-bound, no deep stacks
            .spawn(move || serve_peer_tcp(dir, node_id, enc_key, schema, policy, permit, pt))
        {
            eprintln!("thread spawn failed for peer {}: {e}", peer_id.0);
        }
    }
}

// ---- Concurrent TLS serve loop -----------------------------------------------

fn serve_peer_tls(
    dir: PathBuf,
    node_id: NodeId,
    enc_key: Option<EncryptionKey>,
    schema: PayloadSchema,
    policy: AccessPolicy,
    permit: SemaphorePermit,
    mut pt: TlsPeerTransport,
) {
    let _permit = permit;
    let peer_id = pt.peer_id();
    let mut engine = match open_for_serve(&dir, node_id, &enc_key, &schema, &policy) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("TLS peer {}: engine open error: {e}", peer_id.0);
            return;
        }
    };
    match SyncSession::new(&mut engine, &mut pt).serve_one(peer_id) {
        Ok(stats) => println!(
            "TLS peer {} done: sent={} received={}",
            peer_id.0, stats.events_sent, stats.events_received
        ),
        Err(e) => eprintln!("TLS peer {} sync error: {e}", peer_id.0),
    }
}

fn tls_loop(
    node_id: NodeId,
    dir: &Path,
    enc_key: Option<EncryptionKey>,
    schema: PayloadSchema,
    policy: AccessPolicy,
    max_peers: usize,
    transport: &mut TlsTcpTransport,
) {
    let sem = Semaphore::new(max_peers);
    loop {
        let pt = match transport.accept_split() {
            Ok(pt) => pt,
            Err(e) => {
                eprintln!("TLS accept error: {e}");
                continue;
            }
        };

        let permit = sem.acquire();

        let dir = dir.to_path_buf();
        let enc_key = enc_key.clone();
        let schema = schema.clone();
        let policy = policy.clone();
        let peer_id = pt.peer_id();

        if let Err(e) = std::thread::Builder::new()
            .name(format!("tls-peer-{}", peer_id.0))
            .stack_size(512 * 1024)
            .spawn(move || serve_peer_tls(dir, node_id, enc_key, schema, policy, permit, pt))
        {
            eprintln!("TLS thread spawn failed for peer {}: {e}", peer_id.0);
        }
    }
}
