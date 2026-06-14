mod cmd;
mod metrics;
mod util;

use std::env;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    zamsync_network::tls::install_crypto_provider();

    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("info") => cmd::info(&args),
        Some("submit") => cmd::submit(&args),
        Some("sync") => cmd::sync(&args),
        Some("serve") => cmd::serve(&args),
        Some("compact") => cmd::compact(&args),
        Some("keygen") => cmd::keygen(&args),
        Some("sign") => cmd::sign(&args),
        Some("rekey") => cmd::rekey(&args),
        Some("bench") => cmd::bench(&args),
        Some("daemon") => cmd::daemon(&args),
        Some("audit") => cmd::audit(&args),
        _ => {
            cmd::usage();
            std::process::exit(1);
        }
    }
}
