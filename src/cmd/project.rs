use crate::color;
use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir, open_engine};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use zamsync_core::Event;
use zamsync_storage::PayloadSchema;

use super::project_pg;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let enc_key = load_encryption_key(args)?;
    let dry_run = args.contains(&"--dry-run".to_string());
    let batch_size: usize = flag_value(args, "--batch-size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let only_node: Option<u32> = flag_value(args, "--node").and_then(|v| v.parse().ok());
    let since_ms: Option<u64> = flag_value(args, "--since").and_then(|v| v.parse().ok());
    let format = flag_value(args, "--format").unwrap_or("text");

    let target = flag_value(args, "--target");
    let is_pg =
        matches!(target, Some(u) if u.starts_with("postgres://") || u.starts_with("postgresql://"));

    let node_id = node_id_from_dir(&dir);
    let engine = open_engine(&dir, node_id, enc_key, PayloadSchema::None)?;
    let events = collect_filtered(engine.sorted_scan()?, since_ms, only_node);

    if dry_run {
        let db_label: String = if is_pg {
            target.unwrap().to_string()
        } else {
            resolve_db_path(args, &dir)?.display().to_string()
        };
        eprintln!("{}  would project to {}", color::dim("[dry-run]"), db_label,);

        for event in &events {
            if format == "json" {
                println!(
                    r#"{{"node":{},"seq":{},"hlc_ms":{},"type":{},"size":{}}}"#,
                    event.origin_node.0,
                    event.seq.0,
                    event.hlc.physical,
                    event.event_type,
                    event.payload.len(),
                );
            } else {
                println!(
                    "{}  {}  type={}  {}",
                    color::dim(&format!("node={}", event.origin_node.0)),
                    color::dim(&format!("seq={}", event.seq.0)),
                    event.event_type,
                    color::dim(&format!("size={}B", event.payload.len())),
                );
            }
        }

        let summary = format!("{} events would be projected", events.len());
        if format == "json" {
            eprintln!("{}", color::dim(&format!("[dry-run]  {summary}")));
        } else {
            println!(
                "{}  {}",
                color::dim("[dry-run]"),
                color::yellow(&events.len().to_string()),
            );
        }
        return Ok(());
    }

    if is_pg {
        let url = target.unwrap();
        println!("{}  {}", color::dim("projecting to"), url);
        let iter = events
            .into_iter()
            .map(|e| -> Result<Event, Box<dyn std::error::Error>> { Ok(e) });
        return project_pg::run(url, iter, batch_size);
    }

    let db_path = resolve_db_path(args, &dir)?;
    println!("{}  {}", color::dim("projecting to"), db_path.display());

    let mut conn = Connection::open(&db_path)?;
    init_schema(&conn)?;

    let mut projected = 0usize;
    let mut skipped = 0usize;
    let mut batch: Vec<Event> = Vec::with_capacity(batch_size);

    for event in events {
        batch.push(event);
        if batch.len() >= batch_size {
            let (p, s) = flush_batch(&mut conn, &batch)?;
            projected += p;
            skipped += s;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        let (p, s) = flush_batch(&mut conn, &batch)?;
        projected += p;
        skipped += s;
    }
    println!(
        "{}  {} projected, {} already present",
        color::dim("project  :"),
        color::green(&projected.to_string()),
        color::dim(&skipped.to_string()),
    );

    Ok(())
}

fn collect_filtered(
    scan: impl Iterator<Item = zamsync_core::ZamResult<Event>>,
    since_ms: Option<u64>,
    only_node: Option<u32>,
) -> Vec<Event> {
    scan.filter_map(|r| r.ok())
        .filter(|e| since_ms.is_none_or(|ms| e.hlc.physical >= ms))
        .filter(|e| only_node.is_none_or(|node| e.origin_node.0 == node))
        .collect()
}

fn resolve_db_path(
    args: &[String],
    data_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    match flag_value(args, "--target") {
        None => Ok(data_dir.join("projection.db")),
        Some(url) if url.starts_with("sqlite://") => Ok(sqlite_url_path(url)),
        Some(path) => Ok(PathBuf::from(path)),
    }
}

fn sqlite_url_path(url: &str) -> PathBuf {
    let path = url.trim_start_matches("sqlite://");
    PathBuf::from(normalize_sqlite_url_path(path))
}

fn normalize_sqlite_url_path(path: &str) -> &str {
    #[cfg(windows)]
    {
        if is_windows_drive_path_with_leading_slash(path) {
            return &path[1..];
        }
    }

    path
}

#[cfg(windows)]
fn is_windows_drive_path_with_leading_slash(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':'
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS zamsync_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            origin_node_id  INTEGER NOT NULL,
            seq             INTEGER NOT NULL,
            hlc_ms          INTEGER NOT NULL,
            hlc_logical     INTEGER NOT NULL,
            event_type      INTEGER NOT NULL,
            payload         BLOB    NOT NULL,
            projected_at    TEXT    NOT NULL
                            DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            UNIQUE(origin_node_id, seq)
        );
        CREATE INDEX IF NOT EXISTS idx_origin_seq ON zamsync_events(origin_node_id, seq);
        CREATE INDEX IF NOT EXISTS idx_hlc ON zamsync_events(hlc_ms, hlc_logical);",
    )
}

fn flush_batch(
    conn: &mut Connection,
    events: &[Event],
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    let mut projected = 0usize;
    let mut skipped = 0usize;

    for ev in events {
        let affected = tx.execute(
            "INSERT OR IGNORE INTO zamsync_events \
             (origin_node_id, seq, hlc_ms, hlc_logical, event_type, payload) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                ev.origin_node.0 as i64,
                ev.seq.0 as i64,
                ev.hlc.physical as i64,
                ev.hlc.logical as i64,
                ev.event_type as i64,
                &ev.payload,
            ],
        )?;
        if affected > 0 {
            projected += 1;
        } else {
            skipped += 1;
        }
    }

    tx.commit()?;
    Ok((projected, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zamsync_core::{Hlc, NodeId, SequenceNumber};

    fn make_event(node: u32, seq: u64, hlc_ms: u64, payload: &[u8]) -> Event {
        Event {
            origin_node: NodeId(node),
            seq: SequenceNumber(seq),
            hlc: Hlc::new(hlc_ms, 0),
            event_type: 1,
            payload: payload.to_vec(),
        }
    }

    fn events_as_scan(events: Vec<Event>) -> impl Iterator<Item = zamsync_core::ZamResult<Event>> {
        events.into_iter().map(Ok)
    }

    #[test]
    fn test_filter_by_node() {
        let evs = vec![
            make_event(1, 0, 1000, b"a"),
            make_event(2, 0, 1001, b"b"),
            make_event(1, 1, 1002, b"c"),
        ];
        let result = collect_filtered(events_as_scan(evs), None, Some(1));
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|e| e.origin_node.0 == 1));
    }

    #[test]
    fn test_filter_by_since() {
        let evs = vec![
            make_event(1, 0, 500, b"old"),
            make_event(1, 1, 1000, b"exact"),
            make_event(1, 2, 2000, b"new"),
        ];
        let result = collect_filtered(events_as_scan(evs), Some(1000), None);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|e| e.hlc.physical >= 1000));
    }

    #[test]
    fn test_filter_combined() {
        let evs = vec![
            make_event(1, 0, 500, b"old-n1"),
            make_event(2, 0, 500, b"old-n2"),
            make_event(1, 1, 2000, b"new-n1"),
            make_event(2, 1, 2000, b"new-n2"),
        ];
        let result = collect_filtered(events_as_scan(evs), Some(1000), Some(1));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].origin_node.0, 1);
        assert_eq!(result[0].hlc.physical, 2000);
    }

    #[test]
    fn test_filter_none_passes_all() {
        let evs = vec![make_event(1, 0, 100, b"x"), make_event(2, 1, 200, b"y")];
        let result = collect_filtered(events_as_scan(evs), None, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_init_schema_creates_table() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='zamsync_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_init_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
    }

    #[test]
    fn test_flush_batch_inserts_and_skips() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let events = vec![
            make_event(1, 0, 1000, b"hello"),
            make_event(1, 1, 1001, b"world"),
        ];

        let (p, s) = flush_batch(&mut conn, &events).unwrap();
        assert_eq!(p, 2);
        assert_eq!(s, 0);

        let (p2, s2) = flush_batch(&mut conn, &events).unwrap();
        assert_eq!(p2, 0);
        assert_eq!(s2, 2);
    }

    #[test]
    fn test_resolve_db_path_default() {
        let dir = tempdir().unwrap();
        let path = resolve_db_path(&[], dir.path()).unwrap();
        assert_eq!(path, dir.path().join("projection.db"));
    }

    #[test]
    fn test_resolve_db_path_sqlite_url() {
        let dir = tempdir().unwrap();
        let args = vec!["--target".to_string(), "sqlite:///tmp/test.db".to_string()];
        let path = resolve_db_path(&args, dir.path()).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.db"));
    }

    #[test]
    fn test_resolve_db_path_sqlite_windows_drive_url() {
        let dir = tempdir().unwrap();
        let args = vec![
            "--target".to_string(),
            "sqlite:///C:/Users/test/data.db".to_string(),
        ];
        let path = resolve_db_path(&args, dir.path()).unwrap();

        if cfg!(windows) {
            assert_eq!(path, PathBuf::from("C:/Users/test/data.db"));
        } else {
            assert_eq!(path, PathBuf::from("/C:/Users/test/data.db"));
        }
    }

    #[test]
    fn test_resolve_db_path_postgres_errors() {
        let dir = tempdir().unwrap();
        let args = vec!["--target".to_string(), "/tmp/custom.db".to_string()];
        let path = resolve_db_path(&args, dir.path()).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/custom.db"));
    }
}
