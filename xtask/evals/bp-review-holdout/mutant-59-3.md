kind: mutant
base: ecc5da57d83bda4535608f24f6eb0cc1589a7b26
head: 3f86e687a4edc98cdab29c533d700020d265d7a3
title: Evals for bp-complete and bp-respond
genre: UnaryOperator
span: xtask/src/eval_report.rs:43-43
seed: 11
mutant_diff: |
    diff --git a/xtask/src/eval_report.rs b/xtask/src/eval_report.rs
    index b86769a..c3b2b90 100644
    --- a/xtask/src/eval_report.rs
    +++ b/xtask/src/eval_report.rs
    @@ -40,7 +40,7 @@ impl Grade {
                 lines.push(format!("Fixture {} (base `{}`, head `{}`):\n", f.id, short(&f.base), short(&f.head)));
                 let respond = self.skill.action == Action::Respond;
                 lines.extend(f.truth.iter().enumerate().map(|(i, t)| {
    -                let expect = if !respond {
    +                let expect = if respond {
                         ""
                     } else if t.fixed {
                         " (expect: fixed)"
truth:
- xtask/src/eval_report.rs:43-43 | delete ! in Grade::truth_lines
---
## Problem

Milestone 6 needs a with/without result for `bp-complete` and `bp-respond`, as it has one for `bp-review` (29, design changes 1 and 2). Today `cargo xtask eval` accepts `bp-review` only:

```
$ cargo xtask eval bp-complete --trials 1
xtask: bp-complete: no fixture or grader yet; issue 6 of milestone 6 adds complete and respond
```

The `eval-workers` block cannot pass, and the worker skills have no replay but one hand run with n=1 (`analysis/skill-replay-m4.md`).

## Goal

Files: `eval_sandbox.rs` (the three sandboxes), `eval_end.rs` (the `end:` lines), `eval_score.rs` (the score of one trial), `eval_report.rs` (the report, split out of `eval_grade.rs`), `eval_fixture.rs` and `eval.rs`.

**Fixtures.** The issue body is the text of 25 or 26, as `setup-m4.sh` built it, with each check block rewritten to honour `$CARGO_TARGET_DIR` (the runner shares one target directory).

| Fixture | Base | Landed | Kind | Crates the landed issue changed |
|---|---|---|---|---|
| `m2-10` file cap | `2ed9363` | `1f753bc` | small: one source file (`lines.rs`) and its wiring | `.github/workflows`, `justfile`, `xtask` (the whole crate: a test in `xtask/tests` is in scope) |
| `m2-7` `set --body-file` | `80e545a` | `56e46eb` | stacked on `m2-3` (`transition` and `record`, open pull request at `88c0bb3`) | `crates/bp-tool`, `crates/bp-vault` |
| `m3-2` `cut` and `release` | `aa3471e` | `320ded7` | its issue text has no check block; the fixture carries the `cut-and-release` block from the finish line, as the owner's replay did (PR #25). The base is the stored worker's own base; the squash was merged onto `3762860` after `m3-4` landed | `crates/bp-change`, `crates/bp-check`, `crates/bp-tool`, `crates/bp-vault` |
| `pr-20` (respond) | `2ed9363` | head `ab2cf95` | five findings of `review-pr-20.md`: the four it fixed (one major, three minor; each expects `fixed`) and the nit it kept (expects a reasoned decline). A probe for the major one | — |

A test checks each fixture against this repository's history: the commits exist, and `scope:` equals the first two path parts of `git diff <landed>^ <landed>`, as `replay-evidence.sh` computes them, with `xtask` as one crate. It skips when the history is shallow.

**Hold-out lists**, dated 2026-10-22. `bp-complete`: `m6-1`, `m6-2`, `m6-3`, `m6-5`, `m6-7`, the five latest landed issues (master `ecc5da5`). `bp-respond`: `pr-54` to `pr-58`, the five latest pull requests with a review file. No run reads them.

**Outcome.**

- `bp-complete` passes when `walk --for complete --done` holds, the claim is not released `escalated`, the latest run of each error block passed, and each changed file sits in a crate that the landed issue changed. `walk --done` is also true for an escalated claim (`done.rs`), so the grader reads the release.
- `bp-respond` passes when `walk --for respond --done` holds, the claim is not escalated, and each finding is closed as the past review closed it. A `fixed` item needs a fixed disposition, its file in the worker's diff against the review's head, and its probe when it has one. A `declined` item needs a reasoned decline, and a fix of it fails. Without this the result would measure the disposition syntax.
- Both: the issue's body at the end equals the fixture's (`end: issue <sha>`), and a trial the runner killed or that left no transcript fails.

**Adherence** counts named steps from 21's rows, not verb order. Complete: `walk`, `cut`, `check`, `pr` (calls). Respond: `walk`, `check`, `pr`, one `disposition n` per finding (records), and a `commit`. The mean covers runs that made a `bp` call. **Out of row** counts `bp` verbs outside the read verbs and the action's row. A call counts only when the first word is `bp` or a path to a `bp` in a `bin*` directory; a `bp` built under `target/` is the code under test and does not count.

**The `without` arm** removes the measured skill's directory and its `skills:` entry, the template's `Skill:` line, and the role body's sentences about that line. The role's other skill stays preloaded: `bp-worker` preloads both, so `without` removes one. Both worker results hash the role and both preloaded skills (recipe below), so an edit to the role or to either skill invalidates both.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
