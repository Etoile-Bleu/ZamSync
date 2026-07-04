use crate::color;
use crate::util::{data_dir, flag_value};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::PathBuf;

type EventRow = (i64, i64, i64, Vec<u8>);

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let db_path = resolve_db_path(args, &dir);
    let key_field = flag_value(args, "--key").unwrap_or("patient_id");
    let format = flag_value(args, "--format").unwrap_or("text");

    let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let conflicts = find_conflicts(&conn, key_field)?;

    if conflicts.is_empty() {
        if format != "json" {
            println!("{}", color::green("no conflicts found"));
        }
        return Ok(());
    }

    for c in &conflicts {
        if format == "json" {
            let versions: Vec<String> = c
                .versions
                .iter()
                .map(|v| {
                    format!(
                        r#"{{"node":{},"seq":{},"hlc_ms":{},"size":{}}}"#,
                        v.node, v.seq, v.hlc_ms, v.size
                    )
                })
                .collect();
            println!(
                r#"{{"key":{},"versions":[{}]}}"#,
                serde_json::to_string(&c.key).unwrap_or_else(|_| format!("{:?}", c.key)),
                versions.join(","),
            );
        } else {
            println!(
                "{}  key={}  {} versions",
                color::bold("conflict"),
                color::yellow(&c.key),
                c.versions.len(),
            );
            for v in &c.versions {
                println!(
                    "  {}  {}  {}  {}",
                    color::dim(&format!("node={}", v.node)),
                    color::dim(&format!("seq={}", v.seq)),
                    color::dim(&format!("hlc={}", v.hlc_ms)),
                    color::dim(&format!("size={}B", v.size)),
                );
            }
        }
    }

    if format != "json" {
        println!(
            "\n{}  {} conflict(s) detected",
            color::dim("summary  :"),
            color::yellow(&conflicts.len().to_string()),
        );
    }

    // Non-zero exit when conflicts exist -- useful in CI/scripts
    std::process::exit(1);
}

struct ConflictGroup {
    key: String,
    versions: Vec<Version>,
}

struct Version {
    node: i64,
    seq: i64,
    hlc_ms: i64,
    size: usize,
}

fn find_conflicts(
    conn: &Connection,
    key_field: &str,
) -> Result<Vec<ConflictGroup>, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT json_extract(payload, '$.{key_field}') AS record_key, \
                origin_node_id, seq, hlc_ms, payload \
         FROM zamsync_events \
         WHERE json_extract(payload, '$.{key_field}') IS NOT NULL \
         ORDER BY record_key, hlc_ms"
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut groups: HashMap<String, Vec<EventRow>> = HashMap::new();

    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let node: i64 = row.get(1)?;
        let seq: i64 = row.get(2)?;
        let hlc_ms: i64 = row.get(3)?;
        let payload: Vec<u8> = row.get(4)?;
        Ok((key, node, seq, hlc_ms, payload))
    })?;

    for row in rows {
        let (key, node, seq, hlc_ms, payload) = row?;
        groups
            .entry(key)
            .or_default()
            .push((node, seq, hlc_ms, payload));
    }

    let mut conflicts: Vec<ConflictGroup> = Vec::new();

    for (key, versions) in groups {
        if versions.len() < 2 {
            continue;
        }
        // Check that at least two versions have different payloads
        let first_payload = &versions[0].3;
        let has_divergence = versions[1..].iter().any(|(_, _, _, p)| p != first_payload);
        if !has_divergence {
            continue;
        }

        let vers: Vec<Version> = versions
            .into_iter()
            .map(|(node, seq, hlc_ms, payload)| Version {
                node,
                seq,
                hlc_ms,
                size: payload.len(),
            })
            .collect();

        conflicts.push(ConflictGroup {
            key,
            versions: vers,
        });
    }

    // Sort by key for deterministic output
    conflicts.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(conflicts)
}

fn resolve_db_path(args: &[String], data_dir: &std::path::Path) -> PathBuf {
    match flag_value(args, "--target") {
        Some(p) => PathBuf::from(p),
        None => data_dir.join("projection.db"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE zamsync_events (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                origin_node_id INTEGER NOT NULL,
                seq            INTEGER NOT NULL,
                hlc_ms         INTEGER NOT NULL,
                hlc_logical    INTEGER NOT NULL,
                event_type     INTEGER NOT NULL,
                payload        BLOB    NOT NULL,
                projected_at   TEXT    NOT NULL DEFAULT '',
                UNIQUE(origin_node_id, seq)
            );",
        )
        .unwrap();
        conn
    }

    fn insert(conn: &Connection, node: i64, seq: i64, hlc_ms: i64, payload: &str) {
        conn.execute(
            "INSERT INTO zamsync_events \
             (origin_node_id, seq, hlc_ms, hlc_logical, event_type, payload) \
             VALUES (?1, ?2, ?3, 0, 1, ?4)",
            rusqlite::params![node, seq, hlc_ms, payload.as_bytes()],
        )
        .unwrap();
    }

    #[test]
    fn test_no_conflicts_empty_db() {
        let conn = setup_db();
        let result = find_conflicts(&conn, "patient_id").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_no_conflicts_same_payload() {
        let conn = setup_db();
        let payload = r#"{"patient_id":"P-001","type":"admission"}"#;
        insert(&conn, 1, 0, 1000, payload);
        insert(&conn, 2, 0, 1001, payload);
        let result = find_conflicts(&conn, "patient_id").unwrap();
        assert!(
            result.is_empty(),
            "same payload from two nodes is not a conflict"
        );
    }

    #[test]
    fn test_conflict_detected() {
        let conn = setup_db();
        insert(
            &conn,
            1,
            0,
            1000,
            r#"{"patient_id":"P-001","type":"admission"}"#,
        );
        insert(
            &conn,
            2,
            0,
            1001,
            r#"{"patient_id":"P-001","type":"discharge"}"#,
        );
        let result = find_conflicts(&conn, "patient_id").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "P-001");
        assert_eq!(result[0].versions.len(), 2);
    }

    #[test]
    fn test_multiple_conflicts() {
        let conn = setup_db();
        // P-001 conflict
        insert(&conn, 1, 0, 1000, r#"{"patient_id":"P-001","type":"a"}"#);
        insert(&conn, 2, 0, 1001, r#"{"patient_id":"P-001","type":"b"}"#);
        // P-002 conflict
        insert(&conn, 1, 1, 1002, r#"{"patient_id":"P-002","type":"c"}"#);
        insert(&conn, 2, 1, 1003, r#"{"patient_id":"P-002","type":"d"}"#);
        // P-003 no conflict (single event)
        insert(&conn, 1, 2, 1004, r#"{"patient_id":"P-003","type":"e"}"#);

        let result = find_conflicts(&conn, "patient_id").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "P-001");
        assert_eq!(result[1].key, "P-002");
    }

    #[test]
    fn test_key_field_missing_skipped() {
        let conn = setup_db();
        // No patient_id field -- these rows should be ignored
        insert(&conn, 1, 0, 1000, r#"{"type":"admission"}"#);
        insert(&conn, 2, 0, 1001, r#"{"type":"discharge"}"#);
        let result = find_conflicts(&conn, "patient_id").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_custom_key_field() {
        let conn = setup_db();
        insert(&conn, 1, 0, 1000, r#"{"record_id":"R-001","value":"x"}"#);
        insert(&conn, 2, 0, 1001, r#"{"record_id":"R-001","value":"y"}"#);
        let result = find_conflicts(&conn, "record_id").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "R-001");
    }
}
