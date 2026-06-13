use crate::util::{data_dir, load_encryption_key, node_id_from_dir, EventCounter};
use zamsync_storage::ZamEngine;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let payload = args.get(3).ok_or("missing payload")?.as_bytes().to_vec();
    let enc_key = load_encryption_key(args)?;
    let node_id = node_id_from_dir(&dir);
    let mut engine = match enc_key {
        Some(key) => ZamEngine::open_wal_encrypted(&dir, node_id, EventCounter::default(), key)?,
        None => ZamEngine::open_wal(&dir, node_id, EventCounter::default())?,
    };
    let seq = engine.submit(1, payload)?;
    engine.sync()?;
    println!("submitted seq={}", seq.0);
    Ok(())
}
