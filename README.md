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

## Pull requests

The check `bp-gate` (`.github/workflows/bp-gate.yml`) requires the human's signed approval of a high pull request. `bp install` writes the workflow and the line `gate: <sha>` in `.bp/risk.yml`: the Blueprint commit that the check builds `bp` from. Commit both to the default branch from a shell. Here `.gitignore` holds `.bp/`, so use `git add -f .bp/risk.yml`. Until the default branch holds a pin, the check fails on every pull request.

The check stops an agent only with these forge settings. Without them it stops mistakes only.

- The agent's GitHub token is a fine-grained personal access token on this repository only. It has Contents and Pull requests write. It has no Administration, Workflows or Commit statuses permission. The owner's own `gh` login is not the agent's token.
- Branch protection on `master`, or a ruleset with no bypass actor, requires the checks `bp-gate` and CI, with "Do not allow bypassing the above settings".
- On a public repository, the Actions policy allows `pull_request_target` for `.github/workflows/bp-gate.yml`. From 2026-12-02 GitHub blocks that event by default, and then the check never runs.

A pass is not judged again when the rules on the default branch change. The next push to the pull request runs the check again. On a private repository on GitHub's free plan, GitHub refuses branch protection, so `bp-gate` is advisory there.

Every pull request body follows `.github/pull_request_template.md`. `gh pr create --body` does not load the template, so copy its sections into the body.
