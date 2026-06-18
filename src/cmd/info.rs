use crate::util::{data_dir, flag_value, format_date, load_encryption_key, node_id_from_dir};
use zamsync_core::{ports::StateStore, Event, SequenceNumber, ZamResult};
use zamsync_storage::ZamEngine;

// Collects count + oldest/newest HLC timestamps in a single WAL scan.
#[derive(Default)]
struct InfoState {
    count: usize,
    oldest_ms: Option<u64>,
    newest_ms: Option<u64>,
}

impl StateStore for InfoState {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> ZamResult<()> {
        self.count += 1;
        let phys = event.hlc.physical;
        self.oldest_ms = Some(self.oldest_ms.map_or(phys, |o| o.min(phys)));
        self.newest_ms = Some(self.newest_ms.map_or(phys, |n| n.max(phys)));
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let node_id = node_id_from_dir(&dir);
    let enc_key = load_encryption_key(args)?;

    let engine = match enc_key {
        Some(key) => ZamEngine::open_wal_encrypted(&dir, node_id, InfoState::default(), key)?,
        None => ZamEngine::open_wal(&dir, node_id, InfoState::default())?,
    };

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

    let wal_kb = std::fs::metadata(dir.join("events.wal"))
        .map(|m| m.len() / 1024)
        .unwrap_or(0);
    println!("wal size : {} KB", wal_kb);

    match (engine.state().oldest_ms, engine.state().newest_ms) {
        (Some(o), Some(n)) => {
            println!("oldest   : {}", format_date(o));
            println!("newest   : {}", format_date(n));
        }
        _ => {
            println!("oldest   : --");
            println!("newest   : --");
        }
    }

    if let Some(retain) = flag_value(args, "--retain") {
        println!("retain   : {}", retain);
    }

    Ok(())
}
