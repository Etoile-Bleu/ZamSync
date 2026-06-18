use crate::util::{
    data_dir, flag_value, load_encryption_key, load_schema, node_id_from_dir, open_engine,
};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let before_str = flag_value(args, "--before").ok_or("--before YYYY-MM-DD required")?;
    let cutoff_ms = parse_date_ms(before_str)?;
    let min_keep: usize = flag_value(args, "--min-keep")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let dry_run = args.contains(&"--dry-run".to_string());
    let enc_key = load_encryption_key(args)?;
    let schema = load_schema(args)?;
    let node_id = node_id_from_dir(&dir);

    let wal_path = dir.join("events.wal");
    let wal_size = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);

    if dry_run {
        let engine = open_engine(&dir, node_id, enc_key, schema)?;
        let mut would_drop = 0usize;
        let mut payload_bytes = 0u64;
        for res in engine.scan_events()? {
            let event = res?;
            if event.hlc.physical < cutoff_ms {
                would_drop += 1;
                payload_bytes += event.payload.len() as u64;
            }
        }
        println!("dry-run  : {} events would be expired", would_drop);
        println!(
            "payload  : {} KB in expirable payloads",
            payload_bytes / 1024
        );
        println!("wal size : {} KB", wal_size / 1024);
        return Ok(());
    }

    let mut engine = open_engine(&dir, node_id, enc_key, schema)?;
    let (dropped, bytes_freed) = engine.expire_before(cutoff_ms, min_keep)?;
    engine.sync()?;

    if dropped == 0 {
        println!("expire   : nothing to drop (all events newer than cutoff)");
    } else {
        println!(
            "expire   : dropped {} events, freed {} KB",
            dropped,
            bytes_freed / 1024
        );
    }
    Ok(())
}

/// Parse `YYYY-MM-DD` to milliseconds since Unix epoch (UTC midnight).
pub(crate) fn parse_date_ms(s: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() != 3 {
        return Err(format!("expected YYYY-MM-DD, got {:?}", s).into());
    }
    let year: i64 = parts[0].parse()?;
    let month: i64 = parts[1].parse()?;
    let day: i64 = parts[2].parse()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return Err(format!("invalid date: {}", s).into());
    }
    // Julian Day Number formula (Gregorian calendar, no external deps)
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let jdn = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    let days = jdn - 2_440_588; // JDN of 1970-01-01
    if days < 0 {
        return Err("date is before Unix epoch (1970-01-01)".into());
    }
    Ok(days as u64 * 86_400_000)
}

/// Parse a retention duration string like `365d`, `90d` into milliseconds.
pub(crate) fn parse_retain_ms(s: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let s = s.trim();
    if let Some(days_str) = s.strip_suffix('d') {
        let days: u64 = days_str
            .parse()
            .map_err(|_| format!("invalid --retain value: {}", s))?;
        Ok(days * 86_400_000)
    } else {
        Err(format!("--retain expects Nd format (e.g. 365d), got {:?}", s).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_epoch() {
        assert_eq!(parse_date_ms("1970-01-01").unwrap(), 0);
    }

    #[test]
    fn parse_date_known() {
        // 2024-01-01: 19723 days since epoch
        assert_eq!(parse_date_ms("2024-01-01").unwrap(), 19723 * 86_400_000);
    }

    #[test]
    fn parse_date_rejects_bad_format() {
        assert!(parse_date_ms("20240101").is_err());
        assert!(parse_date_ms("2024-13-01").is_err());
        assert!(parse_date_ms("2024-00-01").is_err());
        assert!(parse_date_ms("1969-12-31").is_err());
    }

    #[test]
    fn parse_retain_days() {
        assert_eq!(parse_retain_ms("365d").unwrap(), 365 * 86_400_000);
        assert_eq!(parse_retain_ms("  90d  ").unwrap(), 90 * 86_400_000);
    }

    #[test]
    fn parse_retain_rejects_bad_format() {
        assert!(parse_retain_ms("30").is_err());
        assert!(parse_retain_ms("30m").is_err());
    }
}
