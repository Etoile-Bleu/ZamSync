mod cmd;
mod color;
mod http;
mod metrics;
mod util;

use std::env;
use tracing_subscriber::EnvFilter;

pub(crate) fn version_string() -> String {
    format!(
        "zamsync {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        std::env::consts::OS,
    )
}

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
        Some("setup") => cmd::setup(&args),
        Some("sign") => cmd::sign(&args),
        Some("rekey") => cmd::rekey(&args),
        Some("bench") => cmd::bench(&args),
        Some("daemon") => cmd::daemon(&args),
        Some("ping") => cmd::ping(&args),
        Some("audit") => cmd::audit(&args),
        Some("project") => cmd::project(&args),
        Some("expire") => cmd::expire(&args),
        Some("snapshot") => cmd::snapshot(&args),
        Some("version") | Some("--version") | Some("-V") => {
            println!("{}", version_string());
            Ok(())
        }
        _ => {
            cmd::usage();
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_contains_package_version() {
        let v = version_string();
        assert!(
            v.contains(env!("CARGO_PKG_VERSION")),
            "expected package version in: {v}"
        );
    }

    #[test]
    fn version_string_starts_with_zamsync() {
        assert!(version_string().starts_with("zamsync "));
    }

    #[test]
    fn version_string_contains_arch_and_os() {
        let v = version_string();
        assert!(v.contains(std::env::consts::ARCH), "expected arch in: {v}");
        assert!(v.contains(std::env::consts::OS), "expected OS in: {v}");
    }
}
