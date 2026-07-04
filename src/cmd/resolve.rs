use crate::color;
use crate::util::{data_dir, flag_value, load_encryption_key, node_id_from_dir, open_engine};
use zamsync_core::EVENT_TYPE_CONFLICT_RESOLUTION;
use zamsync_storage::PayloadSchema;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let enc_key = load_encryption_key(args)?;
    let winner_str = flag_value(args, "--winner").ok_or("--winner <seq>@<node> required")?;
    let (winner_seq, winner_node) = parse_winner(winner_str)?;

    let node_id = node_id_from_dir(&dir);
    let mut engine = open_engine(&dir, node_id, enc_key, PayloadSchema::None)?;

    let payload =
        format!(r#"{{"winner_seq":{winner_seq},"winner_node":{winner_node}}}"#).into_bytes();

    let seq = engine.submit(EVENT_TYPE_CONFLICT_RESOLUTION, payload)?;
    engine.sync()?;

    println!(
        "{}  seq={}  winner={}@{}",
        color::dim("resolved :"),
        color::green(&seq.0.to_string()),
        winner_seq,
        winner_node,
    );
    Ok(())
}

fn parse_winner(s: &str) -> Result<(u64, u32), Box<dyn std::error::Error>> {
    let (seq_str, node_str) = s
        .split_once('@')
        .ok_or("--winner must be in <seq>@<node> format (e.g. 42@2748582051)")?;
    let seq: u64 = seq_str
        .parse()
        .map_err(|_| format!("invalid seq in --winner: {:?}", seq_str))?;
    let node: u32 = node_str
        .parse()
        .map_err(|_| format!("invalid node in --winner: {:?}", node_str))?;
    Ok((seq, node))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_winner_valid() {
        let (seq, node) = parse_winner("42@2748582051").unwrap();
        assert_eq!(seq, 42);
        assert_eq!(node, 2748582051);
    }

    #[test]
    fn test_parse_winner_zero() {
        let (seq, node) = parse_winner("0@1").unwrap();
        assert_eq!(seq, 0);
        assert_eq!(node, 1);
    }

    #[test]
    fn test_parse_winner_no_at() {
        assert!(parse_winner("42").is_err());
        assert!(parse_winner("bad").is_err());
    }

    #[test]
    fn test_parse_winner_non_numeric_seq() {
        assert!(parse_winner("abc@1").is_err());
    }

    #[test]
    fn test_parse_winner_non_numeric_node() {
        assert!(parse_winner("42@abc").is_err());
    }
}
