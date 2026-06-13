use crate::util::{data_dir, node_id_from_dir, EventCounter};
use zamsync_storage::ZamEngine;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let node_id = node_id_from_dir(&dir);
    let engine = ZamEngine::open_wal(&dir, node_id, EventCounter::default())?;

    println!("node_id  : {}", node_id.0);
    println!("data_dir : {}", dir.display());
    println!("events   : {}", engine.state().count);
    let vv = &engine.replication_state().local_vv;
    if vv.entries.is_empty() {
        println!("vv       : (empty)");
    } else {
        for (node, seq) in &vv.entries {
            println!("vv       : node {} @ seq {}", node, seq.0);
        }
    }
    Ok(())
}
