use crate::util::data_dir;
use zamsync_network::generate_credentials;
use zamsync_storage::EncryptionKey;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let tls_dir = dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    // TLS credentials (mTLS transport)
    let creds = generate_credentials()?;
    std::fs::write(tls_dir.join("ca.crt"), &creds.ca_cert_pem)?;
    std::fs::write(tls_dir.join("ca.key"), &creds.ca_key_pem)?;
    std::fs::write(tls_dir.join("node.crt"), &creds.node_cert_pem)?;
    std::fs::write(tls_dir.join("node.key"), &creds.node_key_pem)?;

    // WAL encryption key (at-rest protection)
    let enc_key = EncryptionKey::generate()?;
    enc_key.to_file(tls_dir.join("data.key"))?;

    println!("Credentials generated in {}/tls/", dir.display());
    println!();
    println!("  === TLS (transport encryption) ===");
    println!("  ca.crt   -- copy to all other nodes in this deployment");
    println!("  ca.key   -- keep secret; only needed to sign new node certs");
    println!("  node.crt -- this node's identity certificate");
    println!("  node.key -- this node's private key (never share)");
    println!();
    println!("  === WAL encryption (at-rest) ===");
    println!("  data.key -- 32-byte random key for WAL encryption");
    println!("              CRITICAL: store outside the data dir in production");
    println!("              Recommended: move to /etc/zamsync/data.key (chmod 600)");
    println!();
    println!("Use '--tls' to encrypt transport, '--key-file <path>' to encrypt WAL.");
    Ok(())
}
