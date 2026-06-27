use crate::color;
use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir, open_engine};
use sha2::{Digest, Sha256};
use zamsync_core::Event;
use zamsync_storage::PayloadSchema;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let enc_key = load_encryption_key(args)?;
    let format = flag_value(args, "--format").unwrap_or("text");
    let since_ms: Option<u64> = flag_value(args, "--since").and_then(|v| v.parse().ok());
    let only_node: Option<u32> = flag_value(args, "--node").and_then(|v| v.parse().ok());
    let head: Option<usize> = flag_value(args, "--head").and_then(|v| v.parse().ok());
    let tail: Option<usize> = flag_value(args, "--tail").and_then(|v| v.parse().ok());

    if head.is_some() && tail.is_some() {
        return Err("--head and --tail cannot be used together".into());
    }

    let node_id = node_id_from_dir(&dir);
    let engine = open_engine(&dir, node_id, enc_key, PayloadSchema::None)?;

    let events = collect_filtered(engine.sorted_scan()?, since_ms, only_node);

    let window: &[Event] = match (head, tail) {
        (Some(n), _) => &events[..n.min(events.len())],
        (_, Some(n)) => &events[events.len().saturating_sub(n)..],
        (None, None) => &events,
    };

    print_header(format);

    for event in window {
        print_event(format, event);
    }

    print_footer(format, window.len());

    Ok(())
}

fn collect_filtered(
    scan: impl Iterator<Item = zamsync_core::ZamResult<Event>>,
    since_ms: Option<u64>,
    only_node: Option<u32>,
) -> Vec<Event> {
    scan.filter_map(|r| r.ok())
        .filter(|e| since_ms.is_none_or(|since| e.hlc.physical >= since))
        .filter(|e| only_node.is_none_or(|node| e.origin_node.0 == node))
        .collect()
}

fn print_header(format: &str) {
    match format {
        "text" => {
            println!(
                "{}",
                color::bold(&format!(
                    "{:<27} {:>10} {:>10} {:>6} {:>8}  sha256",
                    "timestamp", "node", "seq", "type", "size"
                ))
            );
            println!("{}", color::dim(&"-".repeat(90)));
        }
        "csv" => println!("timestamp,node,seq,type,size_bytes,sha256"),
        _ => {}
    }
}

fn print_event(format: &str, event: &Event) {
    let ts = unix_ms_to_iso(event.hlc.physical);
    let hash = sha256_hex(&event.payload);

    match format {
        "json" => println!(
            r#"{{"ts":"{ts}","ts_ms":{},"node":{},"seq":{},"type":{},"size":{},"sha256":"{hash}","hlc_logical":{}}}"#,
            event.hlc.physical,
            event.origin_node.0,
            event.seq.0,
            event.event_type,
            event.payload.len(),
            event.hlc.logical,
        ),
        "csv" => println!(
            "{},{},{},{},{},{}",
            ts,
            event.origin_node.0,
            event.seq.0,
            event.event_type,
            event.payload.len(),
            hash,
        ),
        _ => println!(
            "{}  {:>10}  {:>10}  {:>6}  {:>8}  {}",
            color::dim(&ts),
            event.origin_node.0,
            event.seq.0,
            event.event_type,
            event.payload.len(),
            color::dim(&hash[..16]),
        ),
    }
}

fn print_footer(format: &str, count: usize) {
    if format == "text" {
        println!("{}", color::dim(&"-".repeat(90)));
        println!("{} event(s)", color::bold(&count.to_string()));
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let result = h.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

fn unix_ms_to_iso(ms: u64) -> String {
    let secs = ms / 1000;
    let ms_part = ms % 1000;

    let mut days = secs / 86400;
    let time_secs = secs % 86400;
    let hh = time_secs / 3600;
    let mm = (time_secs % 3600) / 60;
    let ss = time_secs % 60;

    let mut year = 1970u32;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }

    let months: [u64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &dm in &months {
        if days < dm {
            break;
        }
        days -= dm;
        month += 1;
    }
    let day = days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}.{ms_part:03}Z")
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_ms_to_iso_epoch() {
        assert_eq!(unix_ms_to_iso(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_unix_ms_to_iso_known_date() {
        assert_eq!(unix_ms_to_iso(1704067200000), "2024-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_unix_ms_to_iso_with_ms() {
        assert_eq!(unix_ms_to_iso(1704067200123), "2024-01-01T00:00:00.123Z");
    }

    #[test]
    fn test_sha256_hex_deterministic() {
        let h1 = sha256_hex(b"patient-record");
        let h2 = sha256_hex(b"patient-record");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_sha256_hex_different_inputs() {
        assert_ne!(sha256_hex(b"record-a"), sha256_hex(b"record-b"));
    }

    #[test]
    fn test_window_head() {
        let events: Vec<u32> = (0..10).collect();
        let n = 3usize;
        let window = &events[..n.min(events.len())];
        assert_eq!(window, &[0, 1, 2]);
    }

    #[test]
    fn test_window_tail() {
        let events: Vec<u32> = (0..10).collect();
        let n = 3usize;
        let window = &events[events.len().saturating_sub(n)..];
        assert_eq!(window, &[7, 8, 9]);
    }

    #[test]
    fn test_window_head_larger_than_total() {
        let events: Vec<u32> = (0..3).collect();
        let n = 10usize;
        let window = &events[..n.min(events.len())];
        assert_eq!(window.len(), 3);
    }

    #[test]
    fn test_window_tail_larger_than_total() {
        let events: Vec<u32> = (0..3).collect();
        let n = 10usize;
        let window = &events[events.len().saturating_sub(n)..];
        assert_eq!(window.len(), 3);
    }

    #[test]
    fn test_window_tail_zero() {
        let events: Vec<u32> = (0..5).collect();
        let n = 0usize;
        let window = &events[events.len().saturating_sub(n)..];
        assert_eq!(window.len(), 0);
    }

    #[test]
    fn test_window_head_zero() {
        let events: Vec<u32> = (0..5).collect();
        let n = 0usize;
        let window = &events[..n.min(events.len())];
        assert_eq!(window.len(), 0);
    }
}
