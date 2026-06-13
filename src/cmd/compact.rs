use crate::util::{data_dir, node_id_from_dir, EventCounter};
use zamsync_storage::ZamEngine;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let node_id = node_id_from_dir(&dir);
    let mut engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;
    let dropped = engine.compact()?;
    engine.sync()?;
    if dropped == 0 {
        println!("nothing to compact (no peers have confirmed events yet)");
    } else {
        println!("compacted: dropped {dropped} WAL records");
    }
    Ok(())
}
