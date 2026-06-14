use crate::util::{data_dir, flag_value};
use zamsync_network::sign_node_cert;
use zamsync_storage::EncryptionKey;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let clinic_dir = data_dir(args, 2)?;

    // Resolve the hub CA cert + key from --ca <hub-dir> or --ca-cert/--ca-key individually
    let (ca_cert_pem, ca_key_pem) = match flag_value(args, "--ca") {
        Some(hub_dir) => {
            let hub_tls = std::path::Path::new(hub_dir).join("tls");
            let cert = std::fs::read_to_string(hub_tls.join("ca.crt"))
                .map_err(|e| format!("read {}/tls/ca.crt: {e}", hub_dir))?;
            let key = std::fs::read_to_string(hub_tls.join("ca.key"))
                .map_err(|e| format!("read {}/tls/ca.key: {e}", hub_dir))?;
            (cert, key)
        }
        None => {
            let cert_path = flag_value(args, "--ca-cert")
                .ok_or("provide --ca <hub-dir> or both --ca-cert <path> and --ca-key <path>")?;
            let key_path = flag_value(args, "--ca-key")
                .ok_or("provide --ca <hub-dir> or both --ca-cert <path> and --ca-key <path>")?;
            let cert =
                std::fs::read_to_string(cert_path).map_err(|e| format!("read {cert_path}: {e}"))?;
            let key =
                std::fs::read_to_string(key_path).map_err(|e| format!("read {key_path}: {e}"))?;
            (cert, key)
        }
    };

    let tls_dir = clinic_dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    let creds = sign_node_cert(&ca_cert_pem, &ca_key_pem)?;
    std::fs::write(tls_dir.join("ca.crt"), &creds.ca_cert_pem)?;
    std::fs::write(tls_dir.join("node.crt"), &creds.node_cert_pem)?;
    std::fs::write(tls_dir.join("node.key"), &creds.node_key_pem)?;

    // Each clinic node gets its own WAL encryption key
    let enc_key = EncryptionKey::generate()?;
    enc_key.to_file(tls_dir.join("data.key"))?;

    println!("Clinic credentials signed in {}/tls/", clinic_dir.display());
    println!();
    println!("  ca.crt   -- hub CA certificate (same as hub's ca.crt)");
    println!("  node.crt -- clinic node certificate, signed by hub CA");
    println!("  node.key -- clinic node private key (never share)");
    println!("  data.key -- WAL encryption key for this clinic node");
    println!();
    println!("Deploy to the clinic device and run:");
    println!(
        "  zamsync serve {0} <bind-addr> --tls --key-file {0}/tls/data.key",
        clinic_dir.display()
    );
    println!(
        "  zamsync sync  {0} <hub-addr>  <hub-id> --tls --key-file {0}/tls/data.key",
        clinic_dir.display()
    );
    Ok(())
}
