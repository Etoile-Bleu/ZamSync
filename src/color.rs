/// Terminal color support for ZamSync CLI output.
///
/// Colors are disabled automatically when:
///   - stdout is not a TTY (piped or redirected)
///   - the `NO_COLOR` environment variable is set (https://no-color.org)
///   - the `TERM` environment variable is set to `dumb`
///   - `--no-color` is present anywhere in the command arguments
///
/// Any of these conditions is sufficient to disable colors.
use std::io::IsTerminal;
use std::sync::OnceLock;

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Returns true if color output is enabled for this process.
pub fn is_enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::io::stdout().is_terminal()
            && std::env::var("NO_COLOR").is_err()
            && std::env::var("TERM").map_or(true, |t| t != "dumb")
            && !std::env::args().any(|a| a == "--no-color")
    })
}

// ---- Core primitive --------------------------------------------------------

/// Apply `code` around `s` when `enabled` is true. Used internally and in tests.
pub(crate) fn paint(s: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("{code}{s}{RESET}")
    } else {
        s.to_owned()
    }
}

// ---- Public color functions ------------------------------------------------

pub fn green(s: &str) -> String {
    paint(s, GREEN, is_enabled())
}

pub fn red(s: &str) -> String {
    paint(s, RED, is_enabled())
}

pub fn yellow(s: &str) -> String {
    paint(s, YELLOW, is_enabled())
}

pub fn bold(s: &str) -> String {
    paint(s, BOLD, is_enabled())
}

pub fn dim(s: &str) -> String {
    paint(s, DIM, is_enabled())
}

// ---- Domain-specific helpers -----------------------------------------------

/// Color an RTT value: green < 100 ms, yellow < 500 ms, red >= 500 ms.
pub fn rtt(ms: u128) -> String {
    rtt_paint(ms, is_enabled())
}

pub(crate) fn rtt_paint(ms: u128, enabled: bool) -> String {
    let s = format!("{ms}ms");
    let code = if ms < 100 {
        GREEN
    } else if ms < 500 {
        YELLOW
    } else {
        RED
    };
    paint(&s, code, enabled)
}

/// Color a packet-loss percentage: green = 0 %, yellow <= 50 %, red > 50 %.
pub fn loss(failures: usize, total: usize) -> String {
    loss_paint(failures, total, is_enabled())
}

pub(crate) fn loss_paint(failures: usize, total: usize, enabled: bool) -> String {
    let pct = (failures * 100).checked_div(total).unwrap_or(0);
    let s = format!("loss={pct}%");
    let code = if pct == 0 {
        GREEN
    } else if pct <= 50 {
        YELLOW
    } else {
        RED
    };
    paint(&s, code, enabled)
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // All tests use the internal `paint` / `*_paint` helpers with an explicit
    // `enabled` flag so they are deterministic regardless of TTY state or
    // environment variables.

    #[test]
    fn paint_enabled_wraps_ansi() {
        let out = paint("hello", GREEN, true);
        assert!(
            out.starts_with("\x1b[32m"),
            "expected green escape, got {out:?}"
        );
        assert!(out.ends_with(RESET), "expected reset suffix");
        assert!(out.contains("hello"), "original text must be present");
    }

    #[test]
    fn paint_disabled_returns_plain() {
        assert_eq!(paint("hello", GREEN, false), "hello");
        assert_eq!(paint("hello", RED, false), "hello");
        assert_eq!(paint("hello", BOLD, false), "hello");
    }

    #[test]
    fn each_color_has_distinct_code() {
        let g = paint("x", GREEN, true);
        let r = paint("x", RED, true);
        let y = paint("x", YELLOW, true);
        let b = paint("x", BOLD, true);
        let d = paint("x", DIM, true);
        // All different from each other
        let variants = [&g, &r, &y, &b, &d];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "color variant {i} and {j} should differ");
                }
            }
        }
    }

    #[test]
    fn rtt_below_100_is_green() {
        let out = rtt_paint(0, true);
        assert!(out.starts_with("\x1b[32m"), "0ms should be green");
        let out = rtt_paint(99, true);
        assert!(out.starts_with("\x1b[32m"), "99ms should be green");
    }

    #[test]
    fn rtt_100_to_499_is_yellow() {
        let out = rtt_paint(100, true);
        assert!(out.starts_with("\x1b[33m"), "100ms should be yellow");
        let out = rtt_paint(499, true);
        assert!(out.starts_with("\x1b[33m"), "499ms should be yellow");
    }

    #[test]
    fn rtt_500_and_above_is_red() {
        let out = rtt_paint(500, true);
        assert!(out.starts_with("\x1b[31m"), "500ms should be red");
        let out = rtt_paint(1200, true);
        assert!(out.starts_with("\x1b[31m"), "1200ms should be red");
    }

    #[test]
    fn rtt_disabled_is_plain_text() {
        assert_eq!(rtt_paint(42, false), "42ms");
        assert_eq!(rtt_paint(250, false), "250ms");
        assert_eq!(rtt_paint(600, false), "600ms");
    }

    #[test]
    fn loss_zero_is_green() {
        let out = loss_paint(0, 3, true);
        assert!(
            out.starts_with("\x1b[32m"),
            "0% loss should be green: {out:?}"
        );
        assert!(out.contains("loss=0%"));
    }

    #[test]
    fn loss_partial_is_yellow() {
        let out = loss_paint(1, 3, true); // 33%
        assert!(
            out.starts_with("\x1b[33m"),
            "33% loss should be yellow: {out:?}"
        );
        let out = loss_paint(1, 2, true); // 50%
        assert!(
            out.starts_with("\x1b[33m"),
            "50% loss should be yellow: {out:?}"
        );
    }

    #[test]
    fn loss_majority_is_red() {
        let out = loss_paint(2, 3, true); // 66%
        assert!(
            out.starts_with("\x1b[31m"),
            "66% loss should be red: {out:?}"
        );
        let out = loss_paint(3, 3, true); // 100%
        assert!(
            out.starts_with("\x1b[31m"),
            "100% loss should be red: {out:?}"
        );
    }

    #[test]
    fn loss_disabled_is_plain_text() {
        assert_eq!(loss_paint(0, 3, false), "loss=0%");
        assert_eq!(loss_paint(3, 3, false), "loss=100%");
    }

    #[test]
    fn loss_zero_total_does_not_panic() {
        let out = loss_paint(0, 0, false);
        assert_eq!(out, "loss=0%");
    }
}
