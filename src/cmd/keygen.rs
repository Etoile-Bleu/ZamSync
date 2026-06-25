use crate::color;
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

    println!(
        "{} {}/tls/",
        color::green("credentials generated in"),
        dir.display()
    );
    println!();
    println!("  {}", color::bold("=== TLS (transport encryption) ==="));
    println!(
        "  {}  copy to all other nodes in this deployment",
        color::dim("ca.crt  --")
    );
    println!(
        "  {}  keep secret; only needed to sign new node certs",
        color::dim("ca.key  --")
    );
    println!(
        "  {}  this node's identity certificate",
        color::dim("node.crt--")
    );
    println!(
        "  {}  this node's private key (never share)",
        color::dim("node.key--")
    );
    println!();
    println!("  {}", color::bold("=== WAL encryption (at-rest) ==="));
    println!(
        "  {}  32-byte random key for WAL encryption",
        color::dim("data.key--")
    );
    println!(
        "              {}  store outside the data dir in production",
        color::yellow("CRITICAL:"),
    );
    println!("              Recommended: move to /etc/zamsync/data.key (chmod 600)");
    println!();
    println!(
        "Use {} to encrypt transport, {} to encrypt WAL.",
        color::bold("--tls"),
        color::bold("--key-file <path>"),
    );
    Ok(())
}
