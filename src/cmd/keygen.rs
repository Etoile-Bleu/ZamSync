use crate::util::data_dir;
use zamsync_network::generate_credentials;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir(args, 2)?;
    let tls_dir = dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    let creds = generate_credentials()?;
    std::fs::write(tls_dir.join("ca.crt"), &creds.ca_cert_pem)?;
    std::fs::write(tls_dir.join("ca.key"), &creds.ca_key_pem)?;
    std::fs::write(tls_dir.join("node.crt"), &creds.node_cert_pem)?;
    std::fs::write(tls_dir.join("node.key"), &creds.node_key_pem)?;

    println!("TLS credentials generated in {}/tls/", dir.display());
    println!();
    println!("  ca.crt   -- copy to all other nodes in this deployment");
    println!("  ca.key   -- keep secret; only needed to sign new node certs");
    println!("  node.crt -- this node's identity certificate");
    println!("  node.key -- this node's private key (never share)");
    println!();
    println!("Use '--tls' with 'serve' and 'sync' to enable encrypted transport.");
    Ok(())
}
