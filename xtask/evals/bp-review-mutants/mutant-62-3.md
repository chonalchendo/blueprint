kind: mutant
base: 7b71a68d92873a4643fe2f2faf3f6b50f522283d
head: b7ae4913b1f535467606e08ca3c441f43b1742ed
title: The trace reader skips heredoc bodies and redirects
genre: BinaryOperator
span: xtask/src/eval_trace.rs:157-157
seed: 7
mutant_diff: |
    diff --git a/xtask/src/eval_trace.rs b/xtask/src/eval_trace.rs
    index 44fa993..c276289 100644
    --- a/xtask/src/eval_trace.rs
    +++ b/xtask/src/eval_trace.rs
    @@ -154,7 +154,7 @@ fn without_redirects(text: &str) -> String {
                 }
                 let op = if b[j..].starts_with(b"&>>") {
                     3
    -            } else if b[j..].starts_with(b"&>") || b[j..].starts_with(b">>") || b[j..].starts_with(b">&") {
    +            } else if b[j..].starts_with(b"&>") || b[j..].starts_with(b">>") && b[j..].starts_with(b">&") {
                     2
                 } else {
                     usize::from(b.get(j) == Some(&b'>') || (b.get(j) == Some(&b'<') && b.get(j + 1) != Some(&b'<')))
truth:
- xtask/src/eval_trace.rs:157-157 | replace || with && in without_redirects
---
## Problem

Issue 9 of milestone 6: `trace-counts-commands`.

## Shape

1. **The eval extends the replay** (17 check 6, 21 check 1; new Eval section). 17 rejects a second replay protocol. This is not one. It keeps the protocol: a clone at the recorded base, a fake forge, a scratch vault, a temporary home, the same claim state and stop point, and reads limited to the walk slice. It adds arms, trials and graders, and so replaces the n=1 hand replay as the grader of check 6. The Bash-call-shape and file-set diff stays as the regression diff against the stored run. The trace grader scores named steps (a call made, a record written), not verb order, because verb order carried only dispatch shape [17 E7]. `cargo xtask eval <skill> --trials N` runs arm *with* and arm *without* (skill body and preload removed, always-on section kept). Each arm runs a `bp` built with `BP_BUNDLE_DIR` at the arm's bundle, because the manifest check refuses an edited install. The *without* dispatch drops the `Skill:` line, and its role copy drops the sentence that runs that line; the filler arm keeps both. Each run asserts the skill is loaded in *with* and absent in *without*; a mis-placed skill once read as 0 % recall [RA §5.5 #6]. The runner is in `xtask`, not `bp`: Blueprint never runs an agent [17 Responsibility].

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
