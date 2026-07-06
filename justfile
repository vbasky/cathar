# cathar developer tasks — `just` command runner

_default:
    @just --list

# One-time after cloning: point git at the committed hooks (pre-commit: fmt + clippy)
setup:
    git config core.hooksPath .githooks
    @echo "→ core.hooksPath set to .githooks (pre-commit runs fmt + clippy)"

# Build the whole workspace
build:
    cargo build --workspace

# Build release with LTO
build-release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run clippy with workspace lints, warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# Check formatting without changing files
fmt-check:
    cargo fmt --all --check

# Build documentation
docs:
    cargo doc --no-deps --document-private-items

# The full CI gate, locally: fmt + clippy + test + doc
check-all: fmt-check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

# Run the CLI, passing through args:  just run -- denoise noisy.wav
run *args:
    cargo run -p cathar-cli -- {{args}}

# Generate a test waveform:  just wave  --out test.wav --duration 3 --freq 440
wave *args:
    cargo run -p cathar-cli -- wave {{args}}

# Audit advisories + licenses + bans (requires: cargo install cargo-deny)
deny:
    cargo deny check

# Release a version: bump → test → tag → push (CI builds binaries + GitHub Release).
# Usage: just release 0.2.0     (set PUBLISH=1 to also publish to crates.io)
release version:
    ./scripts/release.sh {{version}}

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update
