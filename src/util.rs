use std::path::{Path, PathBuf};
use zamsync_core::ports::StateStore;
use zamsync_core::{Event, NodeId, SequenceNumber, ZamResult};
use zamsync_network::TlsConfig;
use zamsync_storage::EncryptionKey;

#[derive(Default)]
pub struct EventCounter {
    pub count: usize,
    pub last_seq: Option<SequenceNumber>,
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

pub fn data_dir(args: &[String], pos: usize) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = PathBuf::from(args.get(pos).ok_or("missing data-dir")?);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn node_id_from_dir(dir: &Path) -> NodeId {
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

pub fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

pub fn is_transient(e: &zamsync_core::ZamError) -> bool {
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

pub fn load_tls_config(dir: &Path) -> Result<TlsConfig, Box<dyn std::error::Error>> {
    let tls_dir = dir.join("tls");
    Ok(TlsConfig::from_files(
        tls_dir.join("node.crt"),
        tls_dir.join("node.key"),
        tls_dir.join("ca.crt"),
    )?)
}

pub fn load_encryption_key(args: &[String]) -> Result<Option<EncryptionKey>, Box<dyn std::error::Error>> {
    match flag_value(args, "--key-file") {
        Some(path) => Ok(Some(EncryptionKey::from_file(path)?)),
        None => Ok(None),
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
