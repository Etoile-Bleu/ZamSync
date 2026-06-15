# Contributing to ZamSync

Thanks for your interest! ZamSync targets resource-constrained deployments (Raspberry Pi, 2G networks, rural clinics), so contributions that keep the binary small, dependency-free, and ARM-compatible are especially welcome.

## Prerequisites

- **Rust stable** (1.75+): install via [rustup](https://rustup.rs)
- **Docker**: required for network simulation tests
- **cross** (optional): for ARM cross-compilation (`cargo install cross`)

## Building

```bash
cargo build --release

# ARM cross-compilation (requires Docker + cross)
cross build --release --target aarch64-unknown-linux-musl
cross build --release --target armv7-unknown-linux-musleabihf
```

## Running tests

```bash
# Unit and library integration tests
cargo test --workspace

# CLI integration tests (requires a built binary)
cargo test --features integration --test cli_integration

# Lints and formatting
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

### Network simulation (Docker required)

```bash
# 4 clinics x 500 events, Rural 2G/EDGE profile
docker compose -f tests/docker-compose.network.yml \
  up --build --abort-on-container-exit

# Report is written to tests/results/report.html
```

## Supported targets

| Target | Status |
|--------|--------|
| `x86_64-unknown-linux-musl` | CI-tested |
| `aarch64-unknown-linux-musl` | CI-tested (Raspberry Pi 4) |
| `armv7-unknown-linux-musleabihf` | CI-tested (Raspberry Pi 2/3) |
| `x86_64-pc-windows-msvc` | CI-tested |

macOS is not a supported target. It may work but is not tested in CI.

## PR guidelines

- **One concern per PR.** A bug fix and a refactor go in separate PRs.
- **Include a test** for any new behavior. CLI tests live in `tests/cli_integration.rs` (run with `--features integration`).
- **No new runtime dependencies** unless strictly necessary. ZamSync compiles to a single static binary with zero system dependencies -- keep it that way.
- **Commit messages**: use `type: subject` format (`feat:`, `fix:`, `docs:`, `chore:`, `test:`).

## Good first issues

Issues labelled [`good first issue`](https://github.com/Etoile-Bleu/ZamSync/labels/good%20first%20issue) are scoped, well-documented, and non-blocking. Each one includes the exact file to edit, acceptance criteria, and an effort estimate.

## License

By contributing you agree your work is released under the [MIT License](LICENSE).
