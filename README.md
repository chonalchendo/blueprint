# Blueprint

One evolving blueprint per project: everything an agent needs to build it and everything a human needs to understand it. The command is `bp`. The design that this code implements lives in the redesign workspace (22, the tooling decision; 23, the Rust workspace; 24, milestone 1) until it is converted into this repository's vault.

## Layout

One Cargo workspace. Crates are named `bp-<domain>` and follow the crate rule in `crate-rule.txt`: domain crates are pure and never depend on each other; `bp-vault` is the substrate; `bp-tool` builds `bp`; `bp-view` builds `bp-view`. `cargo xtask deps` refuses any edge outside the rule.

## Work

```
just          # the recipes
just ci       # what CI runs: lint, deps, test, build
just linux    # a static Linux binary from macOS, through zig
```

The toolchain is pinned in `rust-toolchain.toml`. `cargo deny` and `cargo machete` are installed with `cargo install cargo-deny cargo-machete`; `cargo zigbuild` with `cargo install cargo-zigbuild` and `pip install ziglang`.
