# Krill Task Runner
# Install just: cargo install just

# Default recipe - show all available commands
default:
    @just --list

# Build all workspace members
build:
    cargo build --workspace

# Build in release mode
build-release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy linter
lint:
    cargo clippy --workspace -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run all checks (fmt, clippy, test)
check: fmt-check lint test

# Clean build artifacts
clean:
    cargo clean

# Run the daemon (development mode)
run-daemon:
    cargo run --bin krill-daemon

# Run the TUI (development mode)
run-tui:
    cargo run --bin krill-tui

# Build documentation
doc:
    cargo doc --workspace --no-deps --open

# Run integration tests
test-integration:
    cargo test --test '*' --features integration

# Install the binaries locally
install:
    cargo install --path crates/krill-daemon
    cargo install --path crates/krill-tui

# Watch for changes and rebuild
watch:
    cargo watch -x 'build --workspace'

# Watch and run tests
watch-test:
    cargo watch -x 'test --workspace'
