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

`bp record review` accepts any body. A review with no `head:` line, no findings table, an empty row or a location outside the diff is recorded, and the responder meets it later. `bp record disposition --field as=declined` records with no `reason` (`record.rs:122`, the field is optional).

In review eval run 2, 3 of the 8 `with` findings are false (`analysis/evals-skills/bp-review/report.md`). All three flag the exact-line `#[cfg(test)]` match at `xtask/src/lines.rs:20`, which the formatter makes safe. `bp-review` has no do-not-flag list, and `bp-respond` has no rule for a flaky block.

## Shape

| Before | After |
|---|---|
| `record review` records any body | Refuses `finding_invalid` (exit 2): no `head:` line that names the head, no table, a row with an empty cell, a location outside the diff of the head against the claim's base, or a row with no claim to read a diff from. New code: additive. |
| `record disposition --field as=declined` with no `reason` records | Refuses `schema_invalid`, subject `disposition reason`. Breaking for a caller that declines with no reason. |
| `bp-review` has no do-not-flag list | Step 7 writes no finding for a taste, a formatter style or a case the diff handles; one line says "Do not trust the worker's account … Read the diff." |
| `bp-respond` step 5 runs the blocks | It also says a block that fails once and passes on a rerun is a flake: go on, and name the block and its two run ids in the pull request body. |
| `cargo xtask eval --arms` takes only `with`, `without`, `filler` | Another arm (one lower-case word) runs when `--bin <arm>=<bp>` gives its binary. The grade scores it as `with`; the report adds its row. Additive. The `list` arm drops the trust line only. |
| The review eval's trial passes when the reviewer returns | A review eval trial now passes only when `record review` accepts the review. Run 2 and the rerun are not comparable on `outcome` or `recorded:`. The row check cannot move `false_findings`, which reads the trace. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-tool -p xtask
~~~
