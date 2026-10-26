kind: mutant
base: b7ae4913b1f535467606e08ca3c441f43b1742ed
head: 33a51ddbd41cf8e41b5b726bc26f6eb8d413f16e
title: Mutant fixtures inside real diffs, controls, escapes, --fixtures
genre: UnaryOperator
span: xtask/src/eval.rs:199-199
seed: 11
mutant_diff: |
    diff --git a/xtask/src/eval.rs b/xtask/src/eval.rs
    index 8ce8e11..7badb98 100644
    --- a/xtask/src/eval.rs
    +++ b/xtask/src/eval.rs
    @@ -196,7 +196,7 @@ pub fn run(args: &[String]) -> Result<Vec<String>> {
             return Ok(problems);
         }
         let file = results.join(format!("{name}.md"));
    -    if grade.trials.is_empty() && !file.exists() {
    +    if grade.trials.is_empty() && file.exists() {
             eprintln!("eval: no trials in {}", store.display());
             return Ok(Vec::new());
         }
truth:
- xtask/src/eval.rs:199-199 | delete ! in run
---
## Problem

The review eval has one fixture, `pr-20`. Its truth comes from a past review, so a reader can find it by reading the diff. No fixture plants a defect that CI misses, and no fixture measures false rows on a clean pull request. The proof of milestone 7 needs planted defects with known locations (30, design change 1).

## Goal

`xtask/src/mutants.rs` clones the repository into a `bp-mutants` temp directory. For each pull request it finds the squash whose subject ends `(#<n>)`; the squash's parent is the base.

1. `cargo mutants --list --json --in-diff <diff>` gives the candidates. A tests-only diff lists nothing.
2. The CI screen: it plants each candidate and runs `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`. These are cheaper than the tests, so they run first.
3. For each crate with screened candidates, it builds the workspace binaries, then runs `cargo mutants --in-place` with `--test-package` for every touched crate and one `--re ^<name>$` per screened candidate. A `MissedMutant` outcome survives CI.
4. It sorts the survivors, shuffles them with SplitMix64 seeded by `seed + pr`, and keeps at most one per function, up to `--count`.
5. It writes the span's replacement without the tool's marker, commits it as `bp-eval` and records `git diff head HEAD`. The span is recomputed from that diff, so it is a line of the planted head.

- The issue's Problem is the `## Problem` section of the landed pull request's body (`gh pr view <n> --json body`). The Shape is `## What changed`, or `## How it works` when that section has no table. The text is stored in the fixture, so no run needs the network. The control shares it.
- The Check is `cargo test -q -p <crate>...` over the touched crates: the gate that the mutants survived.
- A pull request that keeps no mutant writes no control.
- `--in-place` runs one mutant at a time; `cargo-mutants` refuses `--jobs` with it.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
