use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir, open_engine};
use zamsync_storage::PayloadSchema;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let out_path = flag_value(args, "--output").ok_or("--output <path> required")?;
    let enc_key = load_encryption_key(args)?;
    let node_id = node_id_from_dir(&dir);

    let wal_src = dir.join("events.wal");
    if !wal_src.exists() {
        return Err(format!("WAL not found: {}", wal_src.display()).into());
    }

    // Flush the WAL writer to disk before copying so no buffered data is missed.
    // The engine is dropped here so all file handles are released before the copy.
    {
        let mut engine = open_engine(&dir, node_id, enc_key, PayloadSchema::None)?;
        engine.sync()?;
    }

    let bytes = std::fs::copy(&wal_src, out_path)?;
    println!("snapshot : {} KB written to {}", bytes / 1024, out_path);
    Ok(())
}
