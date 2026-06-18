use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir};
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

/// Convert Unix milliseconds to `YYYY-MM-DD` (UTC) via reverse Julian Day Number.
fn format_date(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let jdn = days + 2_440_588;
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;
    format!("{:04}-{:02}-{:02}", year, month, day)
}
