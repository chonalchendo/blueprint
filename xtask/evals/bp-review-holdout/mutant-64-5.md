kind: mutant
base: 33a51ddbd41cf8e41b5b726bc26f6eb8d413f16e
head: de275f8373b8a7e3d80e29096661410979929903
title: The kill grader and the old arm
genre: BinaryOperator
span: xtask/src/eval_result.rs:158-158
seed: 11
mutant_diff: |
    diff --git a/xtask/src/eval_result.rs b/xtask/src/eval_result.rs
    index 3541bae..b1ffed0 100644
    --- a/xtask/src/eval_result.rs
    +++ b/xtask/src/eval_result.rs
    @@ -155,7 +155,7 @@ pub(crate) fn newcombe((k1, n1): (usize, usize), (k0, n0): (usize, usize)) -> (f
     fn wilson(k: usize, n: usize) -> (f64, f64, f64) {
         let (p, n, z2) = (share(k, n), float(n), 1.96_f64 * 1.96);
         let (centre, spread, scale) =
    -        (p + z2 / (2.0 * n), 1.96 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt(), 1.0 + z2 / n);
    +        (p + z2 / (2.0 * n), 1.96 * (p * (1.0 - p) * n + z2 / (4.0 * n * n)).sqrt(), 1.0 + z2 / n);
         (p, (centre - spread) / scale, (centre + spread) / scale)
     }
truth:
- xtask/src/eval_result.rs:158-158 | replace / with * in wilson
---
## Problem

The review eval grades a row by its words. It cannot grade a planted defect, and it counts a second true row on a matched item as false (run 3: `false_findings` 0.22, both `with` rows true). It has no `old` arm, so the gain over the shipped body cannot be measured. The trace reader misses a `bp` call after `if` or `while`, and a quoted `;` splits a command. PR #63 left three lows: the store-name fallback, whitespace-only lines in `mutant_diff:`, and three escapes on one pull request.

## Goal

- A fixture set with `kind:` is a planted set. A row kills a planted item when it points into the item's span and carries evidence. Evidence is a backticked command of two words or more that opens a Bash command in the trial's transcript: the whole command, or it with trailing arguments trimmed. A read (`git diff`, `git log`, `git show`, `cat`, `ls`, `head`, `tail`, `grep`, `sed`, `rg`, `find`, `wc`) and a span that starts with `-` are not evidence. A design or convention id (`projects/.../designs/...`, `shared/conventions/...`) is evidence. A numbered cite needs an existing `NN-*.md`: `design 21`, or `(17, check 10)`. In the second form, the first word of the section that is not a stop word must start a heading word of the design or be a word of its text. An advised command that the trial never ran is not evidence. The location must be the same file, and a range that overlaps the span. All its ranges together cite fewer than 20 lines. A reversed range (`113-1`) covers nothing. When two items share a span, the row must also hold one of the item's `match:` slots. A kill needs a recorded review.
- A control's row is false unless it is within 3 lines of a `known:` entry, or is in the entry's file and holds its `match:` words. Then it is `known`. A slot word matches at the start of a word, so `if` does not match `different`.
- On every fixture, a row that matches no open item but sits on a matched item is `duplicate`: neither a match nor false. On a planted fixture it points into the item's span. On a past review's fixture it is within 3 lines of the item's line, or in the item's file and holds one of its `match:` slots.
- The row splitter keeps a `|` inside backticks, or an escaped `\|`, in its cell.
- An arm kills an item when more than half of its trials on the fixture killed it. `holdout_gain` is the `with` kills less the `old` kills over the planted items, with the Newcombe hybrid-score 95% interval (Wilson intervals at z = 1.96, both arms over the same n). The grader refuses `status: capability` unless the printed lower bound is above 0. The refusal names the bound. `--grade` then writes nothing. On a planted set, every graded arm must run every fixture the same number of times. Otherwise the grader refuses the store ("the old arm ran 1 of 2 fixtures; rerun it") and writes nothing.
- `--arm <arm>=<git ref>` builds the arm from `git archive <ref> bundle/`, with `BP_BUNDLE_DIR` at it. Its trials carry that ref's hash and a `ref: <sha>` line. They dispatch from that bundle's template and check that body's first line. The sandbox refuses the trial when the installed `SKILL.md` differs from `git show <sha>:bundle/skills/<skill>/SKILL.md`. `--arm` adds the arm to `--arms`. A `--bin` arm stays as it was.
- The trace reader skips `if`, `elif`, `while` and `until`. A new splitter tracks quotes, escapes and comments. Inside `$'…'`, `\'` is an escape. A `$(` or a backtick inside double quotes still opens a substitution.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
