# blueprint — task runner. `just` lists the recipes.

# Build the bp binary in release mode.
build:
    cargo build --release -p bp-tool

# Run every test.
test:
    cargo test --workspace

# Format, lint with clippy as errors, audit dependencies, find unused ones.
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    cargo machete

# The crate rule (23): every workspace edge is in crate-rule.txt. The file cap (22): no non-test file over 500 lines.
deps:
    cargo xtask deps
    cargo xtask lines

# Everything CI runs, in CI's order.
ci: lint deps test build

# A static Linux binary from macOS, through zig.
linux:
    cargo zigbuild --release -p bp-tool --target x86_64-unknown-linux-musl
