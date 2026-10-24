kind: escape
base: 236c243f99e48dfe7dcf5066052735cf9c9c034a
head: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
title: The review eval runner and grader
truth:
- xtask/src/eval_trace.rs:89 | The reader splits a command at every `&`, so `2>&1` breaks a segment at its `&` and `2>` reads as a verb (escape 20; fixed in #62).
---
## Problem

A walkthrough first. By hand, as 17's replay was run: `xtask eval` dispatches `bp-reviewer` on one fixture from `analysis/review-pr-*.md`, with and without `bp-review`, five times each. The owner reads the report and judges two things. Does the ground truth (findings that anvil's reviewers raised and workers fixed) fit how the owner sees that review? Does the with/without gap mean something, or does a model with no skill fail only because it cannot call `bp`? The report scores adherence on runs that reach the tool, so the second case is visible. The issue also runs a filler arm once (equal length, no content) to separate structure from extra context. The owner records `walkthrough: approved` or the friction before issue 4 starts.

## Shape

1. **The eval extends the replay** (17 check 6, 21 check 1; new Eval section). 17 rejects a second replay protocol. This is not one. It keeps the protocol: a clone at the recorded base, a fake forge, a scratch vault, a temporary home, the same claim state and stop point, and reads limited to the walk slice. It adds arms, trials and graders, and so replaces the n=1 hand replay as the grader of check 6. The Bash-call-shape and file-set diff stays as the regression diff against the stored run. The trace grader scores named steps (a call made, a record written), not verb order, because verb order carried only dispatch shape [17 E7]. `cargo xtask eval <skill> --trials N` runs arm *with* and arm *without* (skill body and preload removed, always-on section kept). Each arm runs a `bp` built with `BP_BUNDLE_DIR` at the arm's bundle, because the manifest check refuses an edited install. The *without* dispatch drops the `Skill:` line, and its role copy drops the sentence that runs that line; the filler arm keeps both. Each run asserts the skill is loaded in *with* and absent in *without*; a mis-placed skill once read as 0 % recall [RA §5.5 #6]. The runner is in `xtask`, not `bp`: Blueprint never runs an agent [17 Responsibility].
2. **Results cannot be typed by hand.** Each trial stores its transcript and its vault log under `analysis/evals-skills/<skill>/`, with each transcript's sha256 in the result file. `cargo xtask eval --grade <skill>` is code only. It recomputes `n`, `adherence`, `out_of_row` and `false_findings` from the traces and fails when the file differs. Result lines: `arms:`, `holdout:`, `skill_loaded:`, `hash:` (role file and skill body concatenated, `git hash-object --stdin`), `model:`, `n:`, `baseline:`, `with:`, `uplift:`, `uplift_ci:`, `adherence:`, `out_of_row:`, `false_findings:` (review), `walkthrough:`, `status:`. `walkthrough:` and `status:` are the owner's lines; the runner writes both `pending`. `--grade --check` accepts `pending`, `capability`, `regression` and `fold`, and refuses `capability` when the lower bound of `uplift_ci` is not above 0. Each trial stores the role-and-skill hash it ran with; `hash:` comes from those, and a store with two hashes, or one unlike the current files, refuses. A person with the traces can still forge both. The block stops accident and drift, not fraud.
3. **Bundle quality goals** (17). The nine scenarios of V6. Thresholds are choices until a first run replaces them.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p xtask
~~~
