kind: escape
base: 236c243f99e48dfe7dcf5066052735cf9c9c034a
head: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
title: The review eval runner and grader
truth:
- xtask/src/eval_arms.rs:64-70 | The `without` arm drops the `Skill:` line, so its dispatch has four lines where the always-on rule says five, and the role refuses it (escape 11; fixed in #60).
---
## Problem

Milestone 6 needs a with/without result for `bp-review` (29, design changes 1 to 3). Today no code runs an agent on a fixture, stores its transcript, or grades it. 17 check 6 rests on one hand replay with n=1. Its result file can be typed by hand.

## Shape

All changes are additive, except the crate rule row.

| Before | After |
|---|---|
| No eval runner. | `cargo xtask eval <skill> --trials N [--arms with,without[,filler]] [--fixture <id>]` stores one transcript and one vault log per trial in `$BP_REDESIGN/analysis/evals-skills/<skill>/`. |
| Results typed by hand. | `cargo xtask eval --grade <skill>` writes `analysis/evals-skills/<skill>.md` and `<skill>/report.md`. `--grade --check` exits 2 with one line per differing line. |
| `bp-review` has no fixture. | `xtask/evals/bp-review/pr-20.md` and `holdout.txt` (five ids, dated). |
| `xtask` depends on no crate. | `xtask -> bp-operation` in `crate-rule.txt` (reads `READ_VERBS` and the action rows). `xtask/tests/deps.rs` anchors on the new row. |
| `bp-install/build.rs` embeds `bundle/`. | It embeds `$BP_BUNDLE_DIR` when set. Unset, the build is unchanged. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p xtask
~~~
