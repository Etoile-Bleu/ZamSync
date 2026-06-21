use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir, open_engine};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use zamsync_core::Event;
use zamsync_storage::PayloadSchema;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let enc_key = load_encryption_key(args)?;
    let dry_run = args.contains(&"--dry-run".to_string());
    let batch_size: usize = flag_value(args, "--batch-size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

    let db_path = resolve_db_path(args, &dir)?;

    if dry_run {
        eprintln!("[dry-run] would project to {}", db_path.display());
    } else {
        println!("projecting to {}", db_path.display());
    }

    let node_id = node_id_from_dir(&dir);
    let engine = open_engine(&dir, node_id, enc_key, PayloadSchema::None)?;

    let mut conn = if !dry_run {
        let c = Connection::open(&db_path)?;
        init_schema(&c)?;
        Some(c)
    } else {
        None
    };

    let mut projected = 0usize;
    let mut skipped = 0usize;
    let mut batch: Vec<Event> = Vec::with_capacity(batch_size);

    for result in engine.sorted_scan()? {
        let event = result?;

        if dry_run {
            println!(
                "node={} seq={} type={} size={}B hlc={}",
                event.origin_node.0,
                event.seq.0,
                event.event_type,
                event.payload.len(),
                event.hlc.physical,
            );
            projected += 1;
            continue;
        }

        batch.push(event);
        if batch.len() >= batch_size {
            let (p, s) = flush_batch(conn.as_mut().unwrap(), &batch)?;
            projected += p;
            skipped += s;
            batch.clear();
        }
    }

    if !dry_run {
        if !batch.is_empty() {
            let (p, s) = flush_batch(conn.as_mut().unwrap(), &batch)?;
            projected += p;
            skipped += s;
        }
        println!("{projected} projected, {skipped} already present");
    } else {
        println!("{projected} events would be projected");
    }

    Ok(())
}

fn resolve_db_path(
    args: &[String],
    data_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    match flag_value(args, "--target") {
        None => Ok(data_dir.join("projection.db")),
        Some(url) if url.starts_with("sqlite://") => Ok(sqlite_url_path(url)),
        Some(url) if url.starts_with("postgres://") || url.starts_with("postgresql://") => Err(
            "PostgreSQL target not yet supported; use --target sqlite://path or omit for default"
                .into(),
        ),
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
        init_schema(&conn).unwrap(); // must not error
    }

    #[test]
    fn test_flush_batch_inserts_and_skips() {
        use zamsync_core::{Event, Hlc, NodeId, SequenceNumber};

        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let events = vec![
            Event {
                origin_node: NodeId(1),
                seq: SequenceNumber(0),
                hlc: Hlc::new(1000, 0),
                event_type: 1,
                payload: b"hello".to_vec(),
            },
            Event {
                origin_node: NodeId(1),
                seq: SequenceNumber(1),
                hlc: Hlc::new(1001, 0),
                event_type: 1,
                payload: b"world".to_vec(),
            },
        ];

        let (p, s) = flush_batch(&mut conn, &events).unwrap();
        assert_eq!(p, 2);
        assert_eq!(s, 0);

        // Second flush -- both should be skipped (UNIQUE conflict)
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
        let args = vec![
            "--target".to_string(),
            "postgres://localhost/db".to_string(),
        ];
        assert!(resolve_db_path(&args, dir.path()).is_err());
    }
}
