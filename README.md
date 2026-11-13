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

The check `bp-gate` (`.github/workflows/bp-gate.yml`) requires the human's signed approval of a high pull request. It judges what the head lands: the test merge of the head into the default branch. A high head passes only on a pull request into the default branch. `bp install` writes the workflow, and the line `gate: <sha>` in `.bp/risk.yml` when the file holds no pin. The pin is the Blueprint commit that the check builds `bp` from, and it must be on Blueprint's default branch. Only `bp install --gate <sha>` replaces a pin.

The check stops an agent only with these forge settings. Without them it stops mistakes only.

- The agent's GitHub token is a fine-grained personal access token on this repository only, with Contents and Pull requests write and no other write. It has no Administration, Workflows, Commit statuses or Actions write.
- The owner's own token is out of the agent's reach. The machine account that runs agents has no `gh` login of the owner, and no keychain, keyring or git credential helper that holds a github.com credential of the owner. The owner acts in the browser, or with `gh` on a separate account.
- The repository's default workflow token is read-only. Every workflow that runs the pull request's code also sets `permissions: contents: read`, as `ci.yml` does.
- Branch protection on `master`, or a ruleset with no bypass actor, requires the checks `bp-gate` and CI from GitHub Actions, with "Do not allow bypassing the above settings". It also requires a pull request before merging (zero approvals, so no direct push reaches `master`), branches up to date before merging, and linear history. The repository allows squash merging only, since `bp gate` judges the squash of the head.
- On a public repository, the Actions policy allows `pull_request_target` for `.github/workflows/bp-gate.yml`. From 2026-12-02 GitHub blocks that event by default, and then the check never runs.

Bootstrap, in this order. Until protection is on, `master` takes any merge, so the trust anchor is what it holds on that day.

1. From a shell, commit `.bp/risk.yml` with `signer:` and `gate:` to `master`. Here `.gitignore` holds `.bp/`, so use `git add -f .bp/risk.yml`. Check the pin with `git merge-base --is-ancestor <pin> origin/master` in a clone of Blueprint.
2. Merge the workflow `.github/workflows/bp-gate.yml`.
3. Read `.bp/risk.yml`, `.gitmodules`, `.gitattributes` and `.github/workflows/` on `master`. Then turn on protection.

In Blueprint itself, step 2 comes first: the pin must be the merge of the workflow or a later commit.

A pin move is high, so the human signs it. Before signing, read `git log -p <old>..<new>` in a clone of Blueprint: the check builds that code, and the ancestry check proves only that it is on Blueprint's default branch.

Until the default branch holds a pin, the check fails on every pull request.

A pass is judged against the default branch's head when its run started. With a pull request required and branches up to date, every move of `master` makes an open head out of date, so its next push runs the check again. On a private repository on GitHub's free plan, GitHub refuses branch protection, so `bp-gate` is advisory there.

Every pull request body follows `.github/pull_request_template.md`. `gh pr create --body` does not load the template, so copy its sections into the body.
