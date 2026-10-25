kind: mutant
base: fc51d9b5c2f37edc8a957153b89639f55084b713
head: 7b71a68d92873a4643fe2f2faf3f6b50f522283d
title: Row checks in record, do-not-flag list, a --bin eval arm
genre: BinaryOperator
span: crates/bp-tool/src/review_rows.rs:71-71
seed: 7
mutant_diff: |
    diff --git a/crates/bp-tool/src/review_rows.rs b/crates/bp-tool/src/review_rows.rs
    index 0326da9..57c62f5 100644
    --- a/crates/bp-tool/src/review_rows.rs
    +++ b/crates/bp-tool/src/review_rows.rs
    @@ -68,7 +68,7 @@ fn review(index: &Index, issue: &Id, head: &str, body: &str) -> Result<(), Refus
                 return Err(refuse(&format!("finding {n}"), fix));
             }
             let at = cells[1].trim_matches('`');
    -        if Id::parse(at).is_ok() && index.header(at)?.is_some() {
    +        if Id::parse(at).is_ok() || index.header(at)?.is_some() {
                 continue;
             }
             if diff.is_none() {
truth:
- crates/bp-tool/src/review_rows.rs:71-71 | replace && with || in review
---
## Problem

Runs after issue 1, so the report shows false findings before and after. The row checks and the do-not-flag list ship in one issue. The row check acts at `record`, after the reviewer returns, so it moves `recorded:` and `outcome`, not `false_findings`, which reads the trace. A third arm, `list`, drops the trust line and keeps the list; it says which text change moves the rate. A trial now passes only when `record review` accepts the review, so run 2 and later runs do not compare on `outcome` (review of PR 61).

## Shape

6. **Row checks in `record`** (15). `record review` refuses `finding_invalid`: no `head:` line, no findings table, an empty finding, or a location outside the diff. `record disposition` refuses `as=declined` with no `reason` (today the field is optional, `record.rs:122`). `bp-review` gains the do-not-flag list and a line: do not trust the worker's account. `bp-respond` gains the flake rule. Both have anvil evidence [AE reviewing-pr, responding-to-pr-review, xc-2].

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-tool -p xtask
~~~
