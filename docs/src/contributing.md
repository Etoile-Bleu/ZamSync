# Contributing

ZamSync targets resource-constrained deployments -- Raspberry Pi, 2G networks, rural clinics. Contributions that keep the binary small, dependency-free, and ARM-compatible are especially welcome.

---

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust stable | 1.75+ | [rustup.rs](https://rustup.rs) |
| Docker | any recent | docker.com -- required for network simulation tests |
| `cross` | optional | `cargo install cross` -- for ARM cross-compilation |

---

## Building

```sh
# Debug build
cargo build

# Release build
cargo build --release

# ARM cross-compilation (requires Docker + cross)
cross build --release --target aarch64-unknown-linux-musl
cross build --release --target armv7-unknown-linux-musleabihf
```

---

## Running tests

```sh
# Unit and library integration tests
cargo test --workspace

# CLI integration tests (requires a built binary in target/debug or target/release)
cargo test --features integration --test cli_integration

# Lints (must be clean -- CI fails on any warning)
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --check
```

All four commands must pass before opening a PR. Run them in this order -- `fmt --check` before `clippy` to avoid noise from formatting warnings.

### Network simulation tests

These tests simulate 2G/EDGE conditions using Docker and TC (traffic control) to apply latency, packet loss, and bandwidth caps:

```sh
docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit

# Results written to tests/results/report.html
```

---

## Supported targets

| Target | Status |
|--------|--------|
| `x86_64-unknown-linux-musl` | CI-tested |
| `aarch64-unknown-linux-musl` | CI-tested (Raspberry Pi 4) |
| `armv7-unknown-linux-musleabihf` | CI-tested (Raspberry Pi 2/3) |
| `x86_64-pc-windows-msvc` | CI-tested |

macOS is not a supported target. It may work but is not tested in CI.

---

## Code style

- **No new runtime dependencies** unless strictly necessary. ZamSync compiles to a single static binary with zero system dependencies -- keep it that way.
- **No `unwrap` or `expect` in library code** (`zamsync-core`, `zamsync-storage`, `zamsync-network`). Propagate errors with `?`.
- `clippy -- -D warnings` must pass. Do not suppress warnings with `#[allow(...)]` without a comment explaining why.
- Format with `cargo fmt --all` before every commit.

---

## PR guidelines

- **CI must pass.** Every PR runs `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test --workspace` on all four targets.
- **One concern per PR.** A bug fix and a refactor go in separate PRs.
- **Include a test** for any new behavior. CLI tests live in `tests/cli_integration.rs` (run with `--features integration`). Unit tests go in the same file as the code under test (`#[cfg(test)]` module).
- **Commit messages**: use `type: subject` format -- `feat:`, `fix:`, `docs:`, `chore:`, `test:`.

---

## Good first issues

Issues labelled [`good first issue`](https://github.com/Etoile-Bleu/ZamSync/labels/good%20first%20issue) are scoped, well-documented, and non-blocking. Each one includes the exact file to edit, acceptance criteria, and an effort estimate.

---

## License

By contributing you agree your work is released under the [MIT License](https://github.com/Etoile-Bleu/ZamSync/blob/main/LICENSE).
