use crate::util::{data_dir, load_encryption_key, load_schema, node_id_from_dir, open_engine};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let payload = args.get(3).ok_or("missing payload")?.as_bytes().to_vec();
    let enc_key = load_encryption_key(args)?;
    let schema = load_schema(args)?;
    let node_id = node_id_from_dir(&dir);
    let mut engine = open_engine(&dir, node_id, enc_key, schema)?;
    let seq = engine.submit(1, payload)?;
    engine.sync()?;
    println!("submitted seq={}", seq.0);
    Ok(())
}
