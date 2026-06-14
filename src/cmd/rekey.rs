use crate::util::{data_dir, flag_value};
use std::sync::Arc;
use zamsync_storage::{EncryptionKey, WalScanner, WalWriter};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let wal_path = dir.join("events.wal");

    let old_key_path = flag_value(args, "--old-key").ok_or("--old-key <path> is required")?;
    let new_key_path = flag_value(args, "--new-key").ok_or("--new-key <path> is required")?;

    if !wal_path.exists() {
        return Err(format!("WAL not found: {}", wal_path.display()).into());
    }

    let old_key = Arc::new(EncryptionKey::from_file(old_key_path)?);
    let new_key = Arc::new(EncryptionKey::from_file(new_key_path)?);

    // Read all records with the old key
    let scanner = WalScanner::open_encrypted(&wal_path, Arc::clone(&old_key))?;
    let records: Result<Vec<_>, _> = scanner.scan().collect();
    let records = records.map_err(|e| format!("WAL read error (wrong key?): {e}"))?;

    let tmp_path = wal_path.with_extension("wal.rekey");
    {
        let mut writer = WalWriter::open_encrypted(
            &tmp_path,
            records.first().map(|r| r.seq).unwrap_or_default(),
            Arc::clone(&new_key),
        )?;
        for record in &records {
            writer.append_at_seq(record.seq, &record.payload)?;
        }
        writer.sync()?;
    }

    std::fs::remove_file(&wal_path)?;
    std::fs::rename(&tmp_path, &wal_path)?;

    println!(
        "Re-keyed {} WAL records in {}",
        records.len(),
        wal_path.display()
    );
    println!("Update your --key-file to point to the new key.");
    Ok(())
}
