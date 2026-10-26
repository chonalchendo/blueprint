kind: mutant
base: 236c243f99e48dfe7dcf5066052735cf9c9c034a
head: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
title: The review eval runner and grader
genre: BinaryOperator
span: xtask/src/eval_grade.rs:169-169
seed: 11
mutant_diff: |
    diff --git a/xtask/src/eval_grade.rs b/xtask/src/eval_grade.rs
    index c49f58f..2cb3fa6 100644
    --- a/xtask/src/eval_grade.rs
    +++ b/xtask/src/eval_grade.rs
    @@ -166,7 +166,7 @@ impl Grade {
         /// The lower bound of `uplift_ci`, when both arms ran.
         pub fn uplift_low(&self) -> Option<f64> {
             let (with, without) = (Stats::of(&self.arm("with")), Stats::of(&self.arm("without")));
    -        (with.runs > 0 && without.runs > 0)
    +        (with.runs == 0 && without.runs > 0)
                 .then(|| newcombe((with.passes, with.runs), (without.passes, without.runs)).1)
         }
truth:
- xtask/src/eval_grade.rs:169-169 | replace > with == in Grade::uplift_low
---
## Problem

Milestone 6 needs a with/without result for `bp-review` (29, design changes 1 to 3). Today no code runs an agent on a fixture, stores its transcript, or grades it. 17 check 6 rests on one hand replay with n=1. Its result file can be typed by hand.

## Goal

`xtask/src/eval.rs` parses the arguments and loops. Each trial builds the replay's sandbox (`eval_sandbox.rs`):

1. `xtask scratch` and `xtask forge bp` make the vault copy and the fake forge.
2. The clone is reset to the fixture's base and pushed. The sandbox seeds a milestone and the fixture's issue.
3. `bp install --target claude` runs with every config home inside the forge directory.
4. As `worker`, `bp cut` claims the issue, the worktree fast-forwards to the recorded head, and `bp pr` opens the pull request on the fake forge. `walk --for review` then reads `needs-review`.

The agent is `$BP_EVAL_AGENT`, a shell command run in the clone. It reads the prompt from `$BP_EVAL_PROMPT`. The prompt is the owner's replay line plus the role's dispatch template. The default is `CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0 claude -p "$(cat "$BP_EVAL_PROMPT")" --setting-sources project,local --output-format json --allowedTools "Bash,Read,Grep,Glob,Edit,Write,Task,Agent"`. It is not run here and is unverified. Do not add `--bare`: it skips hooks, subagents and skills. The ceiling variable stops `-p` from dropping a background subagent's result after ten idle minutes; the runner's own limit (`BP_EVAL_TIMEOUT` seconds, default 1200) replaces it. If the command writes `$BP_EVAL_TRANSCRIPT`, that file is the trace. Else the runner takes the newest `subagents/agent-*.jsonl` of the sandbox whose sibling `.meta.json` names the dispatched role. A timeout or a missing transcript is stored as a failed trial (an `eval_failed` record); it counts as a fail and the report names the reason. `--trials N` fills each arm and fixture up to N stored trials, so a retry adds only what is missing. Before a trial the runner asserts the arm in the clone (the skill file and the role's `skills:` line). A warning prints when `ANTHROPIC_API_KEY` is set, because `claude -p` then bills the key.

After the agent exits, the runner plays the session: it records the returned review (`record review`, role `session`). The vault log then shows the record, and `walk --done` is the outcome.

`eval_grade.rs` reads the store only:

- **Outcome.** A run passes when the review is recorded and at least half of the ground truth is found (a choice). **Prose recall** is the same count taken from the final text in any form (file and a line within 3), so a gap in outcome with none in recall means the arm cannot write the shape.
- **Adherence.** Five steps of `bp-review`: `walk --for review`, `list claim`, a `git diff` at the head, `list delta`, and a review of the right shape (`head:` line, one table). The mean covers only runs that made a `bp` call.
- **Out of row.** `bp` verbs outside the read verbs plus the dispatched role's row, from `bp-operation`. The reviewer's row is empty.
- **False findings.** Findings that match no ground-truth item. Each finding goes to its nearest item by line distance and counts for one item; the report names the item beside each match, and the runs with no finding.
- **skill_loaded.** The first line of the skill body must sit in a preload record (`user`, `isMeta`, `<skill-format>`) of `with`, and nowhere in `without` or `filler`. The filler line must sit in a preload of `filler`. A skill the agent read from disk does not count.
- **Interval.** `uplift_ci` is a Newcombe 95% interval. A Wald interval has no width at 0 or 1, and five trials per arm sit there.
- **`hash:`.** `git hash-object --stdin` over the role file and the skill body, written beside each trial as `<trial>.hash` when it runs. The grade refuses (exit 2) a store with two hashes, a hash that differs from the current files, a missing hash or vault log, or trial numbers that are not 1 to N per arm and fixture. The result file holds a sha256 of each transcript and each vault log (`shasum -a 256`; no new dependency).
- **`status:` and `walkthrough:`.** Both are owner lines, written `pending`. `eval-review` fails until the owner sets `status:` to capability, regression or fold. `--check` refuses `capability` when the lower bound of `uplift_ci` is not above 0.

The arm *without* removes the skill directory, its `skills:` entry, the template's `Skill:` line and the role body's sentences about that line (the `filler` arm keeps them). Each arm is its own `bp` build with `BP_BUNDLE_DIR`, because the manifest check refuses an edited install. The `filler` arm keeps the frontmatter and replaces the body with the same number of bytes of "This line holds no instruction." The shipped bundle builds last, so `target/release/bp` ends as shipped.

Non-test files (the cap is 500): `eval.rs` 268 lines, `eval_agent.rs` 31, `eval_arms.rs` 139, `eval_fixture.rs` 225, `eval_grade.rs` 454, `eval_sandbox.rs` 271, `eval_trace.rs` 120. Usage errors exit 4 like every xtask check.

Design defects and choices (each decided from the designs; none edits a design):

- **Fixtures are in `xtask/evals/<skill>/`, not `bundle/evals/`.** A fixture is machine-read test data with a ground truth. The product bundle should not carry it in every binary, and a held-out id has no place there. (`install` would skip `bundle/evals/`; the reason is not that it would ship.)
- **The arm needs its own binary.** 29 says "skill body and preload removed". 17 check 7 refuses an installed file that differs from the embedded one. So the runner builds a `bp` per arm.
- **`install` refuses a template that names an unpreloaded skill.** The arm *without* therefore dispatches with no `Skill:` line. The other four lines match.
- **The filler arm is not in `arms:`, `n:` or `uplift:`.** The block requires `arms: with, without`. Filler trials show in the Trials table and the report.
- **`status:` is an owner line**, like `walkthrough:`, and starts `pending`. A regrade keeps both lines; `--check` refuses a `status` outside `pending` and the three values.
- **`false_findings:` is `n/a` when `with` returned no finding.**
- **Fixture issue.** The body is the issue of 25 (issue 10) with the Shape as the review quoted it before the fix, so the ground truth is not in the text. The block is the real one: `cargo xtask lines`, then the same on a copy with a 501-line file, which must exit 2 (it builds offline in the sandbox on a shared target directory). The issue cites the vault's system design, because 22 is not in the vault copy.
- **Ground truth of PR 20.** The four findings its Disposition table marks Fixed (item 2 is `lines.rs:11-27`, where the test belongs). The two design findings marked Folded are out: the sandbox holds no design 25.
- **Held out:** `pr-52` to `pr-56`, the five latest landed pull requests that have a review file. Edit `holdout.txt` to change it.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
