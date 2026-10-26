kind: mutant
base: 3f86e687a4edc98cdab29c533d700020d265d7a3
head: fc51d9b5c2f37edc8a957153b89639f55084b713
title: Repair the review fixture and the without arm
genre: BinaryOperator
span: xtask/src/eval_report.rs:14-14
seed: 11
mutant_diff: |
    diff --git a/xtask/src/eval_report.rs b/xtask/src/eval_report.rs
    index 576a4ff..586c5c6 100644
    --- a/xtask/src/eval_report.rs
    +++ b/xtask/src/eval_report.rs
    @@ -11,7 +11,7 @@ use crate::eval_score::{ASPECTS, Graded, RECALL};
     
     /// The arms table's last column: how many runs had the skill where the arm puts it.
     fn state(arm: &str, loaded: usize, runs: usize) -> String {
    -    match (arm, loaded == runs) {
    +    match (arm, loaded != runs) {
             ("without", true) => "absent, as designed".into(),
             ("without", false) => format!("present in {} of {runs}", runs - loaded),
             _ => loaded.to_string(),
truth:
- xtask/src/eval_report.rs:14-14 | replace == with != in state
---
## Problem

The first `bp-review` eval run (11 trials, 2026-10-23) measured the instrument, not the skill. The audit is `analysis/evals-skills/bp-review/audit.md`.

- The `without` arm cut the `Skill:` line from the dispatch and the role body. Four of five `without` runs stopped on a four-line dispatch.
- Truth items 2 to 4 rest on rules the reviewer never had. Item 3 cites the wrong lines.
- The grader credited a finding by its place. `with-5` passed on two location matches that are not the truth items by substance.
- The grader called the human review's own finding (the `tests/` skip at `lines.rs:34`) FALSE in five of five `with` runs.
- The 22 cap rule was in no node the walk carried.

## Goal

`xtask/src/eval_fixture.rs`: `Truth` holds lowercase `keywords`. `best` picks the item for one finding or one citation: all keywords in the text, then most keywords, then `apart` (lines apart, then item width). `NEAR` is gone.

`xtask/src/eval_arms.rs`: `arm_bundle` for `without` removes the skill directory, drops the `skills:` entry and rewrites the `Skill:` line of the action's template to `Skill: none`. The sentence-removal helper had no other use, so it is deleted.

`xtask/src/eval_sandbox.rs`: one loop creates each `rule:` design and links it. A convention would carry its id only: the review walk sends the body of a design alone (`Action::bodies`, 21). So the rule is a design.

The match words come from each item's own text, not from the runs. Item 1: `cfg(test)`, and one of `helper|test-only|uncounted|escape|under the cap|below`. Item 2: `check|block`, a boundary or exit-code word, and an untested word (three slots, so most slots decides against item 3). Item 3: `skip`, `tests`, and a Shape or declare word. A test checks that each item's own text, at its own location, is assigned to that item.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p xtask
~~~
