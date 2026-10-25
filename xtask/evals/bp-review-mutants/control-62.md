kind: control
base: 7b71a68d92873a4643fe2f2faf3f6b50f522283d
head: b7ae4913b1f535467606e08ca3c441f43b1742ed
title: The trace reader skips heredoc bodies and redirects
known: xtask/src/eval_trace.rs:99 | The prefix words lack if, elif, while and until, so a bp call after one is not counted (30, design change 5). | match: if|while|until, prefix|keyword
known: xtask/src/eval_trace.rs:96 | A quoted ; still splits the command, so text after it can read as a call or drop one (30, design change 5). | match: quot, ;|semicolon, split
truth:
---
## Problem

The eval grader counts `bp` calls that never ran. `calls_in` in `xtask/src/eval_trace.rs` splits a Bash command at `;`, `&`, `|`, newline, parentheses and backticks. It then counts each segment whose first word is `bp`.

- A heredoc body that holds source code such as `` `bp set <id> ...` `` or `format!("bp record delta ...")` yields calls. The shell never runs them.
- `bp --help 2>&1 | head -40` splits at the `&` of `2>&1`. The reader takes `2>` as the verb of a call.
- In the committed `bp-complete` result, `out_of_row` is 16 in the `with` arm. The finish-line block `eval-workers` of 29 requires 0.

## Shape

| Before | After |
|---|---|
| A line inside a heredoc body that starts with `bp` counts as a call. | The lines from `<<WORD` (also `<<-WORD`, quoted `WORD`) to the line that is `WORD` are skipped. Several heredocs on one line work. A `<<<` here-string opens no body. |
| `2>&1` splits at its `&` and leaves a call with verb `2>`. | The reader removes a redirect (`N>&M`, `>&N`, `&>f`, `&>>f`, `N>f`, `>f`, `>>f`, `<f`) before it splits. |
| The report said a heredoc line counts. | The "Counted by line" note says a heredoc body and a redirect are skipped. |

No contract changes. The `bp` calls that a `$(...)` or a backtick substitution holds still count.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p xtask
~~~
