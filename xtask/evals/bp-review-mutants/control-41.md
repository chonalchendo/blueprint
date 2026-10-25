kind: control
base: c8a37b4eec25e6170f55d241cb0cd38545476503
head: 1f275408c28a70f0dd916decf1df9ce27472af91
title: The design walk under budget
truth:
---
## Problem

25's `call-budget-with-runs` (max 2.5) read 2.50 to 2.51 at master `c8a37b4`. Before issue 12 (PR #40), it read 2.30 to 2.42. The `design` call sets the measure: `bp walk projects/bp/designs/system --for design --json` on a scratch copy with four integrity milestones and their runs. That walk reaches 142 issues, 25 milestones and 61 designs. A profile of the intent loop at `c8a37b4`:

| Nodes | Cost |
|---|---|
| 137 `resolved` issues | 0.50 to 0.72 ms |
| 22 `done` milestones | 0.25 to 0.67 ms |
| 51 `active` designs | 0.10 to 0.14 ms |

A resolved issue and a done milestone have no intent that reads an integrity run. But PR #40 sent `Intents::integrity` through the new `once` cache, which clones the cached value on each call. The counting `bp` integrity run names 103 nodes in a `BTreeSet<String>`. So each issue and milestone row cloned that set, live or not.

## Shape

| Before | After |
|---|---|
| Each issue row read the integrity run, then returned no intent when the issue was settled. | A settled issue returns no intent and reads nothing. No contract change. |
| Each milestone row read the integrity run, whatever its status. | Only a `planned` or `approved` milestone reads it, because blocked holds only before `done` (21). No contract change. |
| `integrity` cloned the project's counting run for each row. | `integrity` reads the run once for each project and answers `blocks(id)` from the cache, without a clone. No contract change. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-tool
~~~
