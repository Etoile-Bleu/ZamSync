use crate::util::{data_dir, flag_value};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let out_path = flag_value(args, "--output").ok_or("--output <path> required")?;

    let wal_src = dir.join("events.wal");
    if !wal_src.exists() {
        return Err(format!("WAL not found: {}", wal_src.display()).into());
    }

    let bytes = std::fs::copy(&wal_src, out_path)?;
    println!("snapshot : {} KB written to {}", bytes / 1024, out_path);
    Ok(())
}
