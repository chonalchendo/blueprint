kind: mutant
base: 7b71a68d92873a4643fe2f2faf3f6b50f522283d
head: b7ae4913b1f535467606e08ca3c441f43b1742ed
title: The trace reader skips heredoc bodies and redirects
genre: FnValue
span: xtask/src/eval_report.rs:138-153
seed: 7
mutant_diff: |
    diff --git a/xtask/src/eval_report.rs b/xtask/src/eval_report.rs
    index 41e950d..d103f85 100644
    --- a/xtask/src/eval_report.rs
    +++ b/xtask/src/eval_report.rs
    @@ -135,22 +135,7 @@ impl Grade {
     
         /// What the columns mean, for the skill. The last two bullets hold for every skill.
         fn notes(&self) -> String {
    -        let own = match self.skill.action {
    -            Action::Review => format!(
    -                "- **Outcome:** a run passes when `bp walk --for review --done` holds after the review is recorded and the findings table matches at least {RECALL} of the ground truth. A finding matches the item whose slots (the `match:` words of the fixture) its text holds, the most slots first, then the nearest line; where it points earns nothing, and a finding with an `unless:` phrase matches nothing. Keywords cannot read a negation or a verdict, so read each match and each FALSE row. That needs the shape only `bp-review` defines (a `head:` line, one table).\n- **Prose recall:** an upper bound, never an outcome. Items of the ground truth that the final text substantiates in any form: a citation of the item's file, with a line number or none, on a line that holds the item's words. A citation earns one item, and a line that states the item is no defect can still earn it when it holds no `unless:` phrase. It does not depend on the shape, so a gap in outcome with none in prose recall means the arm cannot write the shape.\n"
    -            ),
    -            Action::Complete => "- **Outcome:** a run passes when `bp walk --for complete --done` holds with the claim not released `escalated`, the latest run of each error block of the issue passed, and every changed file sits in a crate that the landed issue changed (the first two path parts, as `replay-evidence.sh` compares them). The file set is not compared: the model is not deterministic.\n- **Adherence:** the share of walk, cut, check and pr that the transcript shows as calls, over the runs that reached the tool.\n".to_string(),
    -            _ => "- **Outcome:** a run passes when `bp walk --for respond --done` holds with the claim not released `escalated` and each finding is closed as the past review closed it. A `fixed` finding needs a fixed disposition, its file in the worker's diff against the review's head, and its probe when the fixture gives one. A `declined` finding needs a reasoned decline, and a fix of it fails. The probe and the file are evidence of a fix, not proof of a good one: the owner reads the trace.\n- **Adherence:** the share of walk, check, pr, one closed disposition per finding and a commit that the transcript and the vault show, over the runs that reached the tool.\n".to_string(),
    -        };
    -        let counted = "- **Counted by line:** the adherence and `out of row` columns read `bp` calls from the transcript's Bash commands, one per line, `;`, `&`, `|`, parenthesis or backtick. A heredoc body and a redirect are skipped. A quoted `;` splits wrongly, and a stray `<<` hides a body only when a later line equals the word after it. A `bp` built under `target/` is not counted.\n";
    -        let scope = if self.skill.action == Action::Complete {
    -            "- **Scope:** the unit is the first two path parts, and `xtask` as a whole, so a test file in `xtask/tests` is in scope.\n"
    -        } else {
    -            ""
    -        };
    -        format!(
    -            "\n{own}{scope}{counted}- **Failed trial:** a trial the runner killed at its limit, or that left no transcript, fails whatever the sandbox holds. A rerun of the same command replaces it.\n- **Uplift:** the `with` rate less the `without` rate; the result file gives its Newcombe 95% interval.\n- **Role body:** the `without` arm's role body is the shipped one and its dispatch reads `Skill: none`, so it differs from `with` in the missing body and preload only.\n"
    -        )
    +        String::new()
         }
     
         fn trial_lines(&self, t: &Graded, store: &Path) -> Vec<String> {
truth:
- xtask/src/eval_report.rs:138-153 | replace Grade::notes -> String with String::new()
---
## Problem

Issue 9 of milestone 6: `trace-counts-commands`.

## Shape

1. **The eval extends the replay** (17 check 6, 21 check 1; new Eval section). 17 rejects a second replay protocol. This is not one. It keeps the protocol: a clone at the recorded base, a fake forge, a scratch vault, a temporary home, the same claim state and stop point, and reads limited to the walk slice. It adds arms, trials and graders, and so replaces the n=1 hand replay as the grader of check 6. The Bash-call-shape and file-set diff stays as the regression diff against the stored run. The trace grader scores named steps (a call made, a record written), not verb order, because verb order carried only dispatch shape [17 E7]. `cargo xtask eval <skill> --trials N` runs arm *with* and arm *without* (skill body and preload removed, always-on section kept). Each arm runs a `bp` built with `BP_BUNDLE_DIR` at the arm's bundle, because the manifest check refuses an edited install. The *without* dispatch drops the `Skill:` line, and its role copy drops the sentence that runs that line; the filler arm keeps both. Each run asserts the skill is loaded in *with* and absent in *without*; a mis-placed skill once read as 0 % recall [RA §5.5 #6]. The runner is in `xtask`, not `bp`: Blueprint never runs an agent [17 Responsibility].

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
