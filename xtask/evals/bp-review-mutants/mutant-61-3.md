kind: mutant
base: fc51d9b5c2f37edc8a957153b89639f55084b713
head: 7b71a68d92873a4643fe2f2faf3f6b50f522283d
title: Row checks in record, do-not-flag list, a --bin eval arm
genre: BinaryOperator
span: xtask/src/eval_grade.rs:88-88
seed: 7
mutant_diff: |
    diff --git a/xtask/src/eval_grade.rs b/xtask/src/eval_grade.rs
    index 3d229a5..6aab691 100644
    --- a/xtask/src/eval_grade.rs
    +++ b/xtask/src/eval_grade.rs
    @@ -85,7 +85,7 @@ impl Grade {
                 let log = std::fs::read_to_string(&log_path).unwrap_or_default();
                 let loaded = match arm {
                     "without" => !trace.mentions(&marker) && !trace.mentions(FILLER),
    -                "filler" => trace.preloaded(FILLER) && !trace.mentions(&marker),
    +                "filler" => trace.preloaded(FILLER) || !trace.mentions(&marker),
                     // `with`, or an arm that ran with its own `--bin`: an edited body keeps the body's first line.
                     _ => trace.preloaded(&marker),
                 };
truth:
- xtask/src/eval_grade.rs:88-88 | replace && with || in Grade::read
---
## Problem

Runs after issue 1, so the report shows false findings before and after. The row checks and the do-not-flag list ship in one issue. The row check acts at `record`, after the reviewer returns, so it moves `recorded:` and `outcome`, not `false_findings`, which reads the trace. A third arm, `list`, drops the trust line and keeps the list; it says which text change moves the rate. A trial now passes only when `record review` accepts the review, so run 2 and later runs do not compare on `outcome` (review of PR 61).

## Shape

6. **Row checks in `record`** (15). `record review` refuses `finding_invalid`: no `head:` line, no findings table, an empty finding, or a location outside the diff. `record disposition` refuses `as=declined` with no `reason` (today the field is optional, `record.rs:122`). `bp-review` gains the do-not-flag list and a line: do not trust the worker's account. `bp-respond` gains the flake rule. Both have anvil evidence [AE reviewing-pr, responding-to-pr-review, xc-2].

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-tool -p xtask
~~~
