kind: mutant
base: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
head: ecc5da57d83bda4535608f24f6eb0cc1589a7b26
title: Confine the worker; refuse a verb outside the role's row
genre: BinaryOperator
span: xtask/src/main.rs:58-58
seed: 7
mutant_diff: |
    diff --git a/xtask/src/main.rs b/xtask/src/main.rs
    index c87a9ab..6c10af9 100644
    --- a/xtask/src/main.rs
    +++ b/xtask/src/main.rs
    @@ -55,7 +55,7 @@ fn main() -> ExitCode {
             Some("sandbox") => {
                 let claim = match &args[1..] {
                     [] => false,
    -                [one] if one == "claim" => true,
    +                [one] if one != "claim" => true,
                     _ => {
                         eprintln!("usage: cargo xtask sandbox [claim]");
                         return ExitCode::from(4);
truth:
- xtask/src/main.rs:58-58 | replace == with != in main
---
## Problem

A worker can still write in the main checkout and background a command, and the tool checks no verb against the role. Three prose rules failed in anvil.

- A whole fix landed in the main checkout while the check read green (anvil 0249).
- At least 16 worker stalls came from `run_in_background`, past a prose rule (anvil 0183 and the chain after it).
- A SessionEnd hook that outlasts 1.5 s dies with no message, and `bp-end.sh` has no `timeout`.
- `BP_ROLE=worker bp create inbox --title Q --json` creates the inbox node. It should refuse. Only `stamp`, `land` and a `transition` to `approved` or `active` refuse an agent role today.

## Shape

| Before | After |
|---|---|
| `bp-guard.sh` denies a write in the vault only | It also denies a `bp-worker` Edit or Write inside the clone's main checkout |
| `bp-role.sh` checks `bp` verbs only | It also denies `run_in_background` for `bp-worker`, on any command |
| The SessionEnd entry has no `timeout` | The entry has `"timeout": 10` |
| The tool refuses `stamp`, `land` and an approving `transition` under an agent role | It also refuses every verb outside the row of `BP_ROLE` with the new code `verb_outside_role`. A new code on verbs that ran before: under an agent role, a verb outside its row now refuses. |
| No fixture for a hook block | `cargo xtask sandbox [claim]` prints 27's prelude as `export` lines. With `claim` it adds a held worker claim: `$I`, `$W`, `$G`, `$K`. |

`role_forbidden` keeps its text. `stamp`, `land` and a `transition` to `approved` or `active` still return it (15: a code never changes meaning).

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p bp-tool -p xtask
~~~
