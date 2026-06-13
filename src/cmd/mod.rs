mod bench;
mod compact;
mod info;
mod keygen;
mod serve;
mod submit;
mod sync;

pub use bench::run as bench;
pub use compact::run as compact;
pub use info::run as info;
pub use keygen::run as keygen;
pub use serve::run as serve;
pub use submit::run as submit;
pub use sync::run as sync;

pub fn usage() {
    eprintln!(
        "Usage:
  zamsync info    <data-dir>
  zamsync submit  <data-dir> <payload>
  zamsync sync    <data-dir> <peer-addr> <peer-id> [--tls] [--metrics <addr>]
  zamsync serve   <data-dir> <bind-addr> [--tls] [--metrics <addr>]
  zamsync compact <data-dir>
  zamsync keygen  <data-dir>
  zamsync bench   <data-dir> [--events N]

Flags (serve / sync):
  --tls            Use mTLS with credentials in <data-dir>/tls/
                   Run 'zamsync keygen' first to generate credentials.
  --metrics <addr> Expose Prometheus /metrics on <addr> (e.g. 0.0.0.0:9090)"
    );
}
