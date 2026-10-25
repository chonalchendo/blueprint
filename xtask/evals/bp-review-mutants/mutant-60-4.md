kind: mutant
base: 3f86e687a4edc98cdab29c533d700020d265d7a3
head: fc51d9b5c2f37edc8a957153b89639f55084b713
title: Repair the review fixture and the without arm
genre: BinaryOperator
span: xtask/src/eval_report.rs:14-14
seed: 7
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

The first real run (2026-10-23, 11 trials) measured the instrument, not the skill: `analysis/evals-skills/bp-review.run1/audit.md`. The `without` arm refused a four-line dispatch (`AGENTS.md` says five); truth items 2 to 4 rest on rules absent from the reviewer's context; item 3 cites the wrong lines; the one `with` pass is by location, not substance; the grader called the human review's own design-doc finding FALSE five times. Repair per the audit's nine points, then the owner reruns the review eval. Issue 4 waits for the rerun. Rules from PR 60's review: a truth item's `match:` words are slots of alternatives drawn from the truth text, never from graded runs, with `unless:` words that void a match; one item per finding, most slots first, line distance as the tie-break; each trial's `.hash` also stamps the fixture and the arm protocol; prose recall is an upper bound, never an outcome. Item 2 reads: the Check block tests one exit code on one 501-line file; the printed path and count, the 500 boundary, the `tests/` skip and the cut-off are untested.

## Shape

1. **The eval extends the replay** (17 check 6, 21 check 1; new Eval section). 17 rejects a second replay protocol. This is not one. It keeps the protocol: a clone at the recorded base, a fake forge, a scratch vault, a temporary home, the same claim state and stop point, and reads limited to the walk slice. It adds arms, trials and graders, and so replaces the n=1 hand replay as the grader of check 6. The Bash-call-shape and file-set diff stays as the regression diff against the stored run. The trace grader scores named steps (a call made, a record written), not verb order, because verb order carried only dispatch shape [17 E7]. `cargo xtask eval <skill> --trials N` runs arm *with* and arm *without* (skill body and preload removed, always-on section kept). Each arm runs a `bp` built with `BP_BUNDLE_DIR` at the arm's bundle, because the manifest check refuses an edited install. The *without* dispatch drops the `Skill:` line, and its role copy drops the sentence that runs that line; the filler arm keeps both. Each run asserts the skill is loaded in *with* and absent in *without*; a mis-placed skill once read as 0 % recall [RA §5.5 #6]. The runner is in `xtask`, not `bp`: Blueprint never runs an agent [17 Responsibility].
2. **Results cannot be typed by hand.** Each trial stores its transcript and its vault log under `analysis/evals-skills/<skill>/`, with each transcript's sha256 in the result file. `cargo xtask eval --grade <skill>` is code only. It recomputes `n`, `adherence`, `out_of_row` and `false_findings` from the traces and fails when the file differs. Result lines: `arms:`, `holdout:`, `skill_loaded:`, `hash:` (role file and skill body concatenated, `git hash-object --stdin`), `model:`, `n:`, `baseline:`, `with:`, `uplift:`, `uplift_ci:`, `adherence:`, `out_of_row:`, `false_findings:` (review), `walkthrough:`, `status:`. `walkthrough:` and `status:` are the owner's lines; the runner writes both `pending`. `--grade --check` accepts `pending`, `capability`, `regression` and `fold`, and refuses `capability` when the lower bound of `uplift_ci` is not above 0. Each trial stores the role-and-skill hash it ran with; `hash:` comes from those, and a store with two hashes, or one unlike the current files, refuses. A person with the traces can still forge both. The block stops accident and drift, not fraud.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p xtask
~~~
