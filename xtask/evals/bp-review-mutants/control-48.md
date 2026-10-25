kind: control
base: 2e6f25a1ed7779fb8067dd94aa311502b2f2c67d
head: 1ba38a9a50a6c26e86b56b4196f1bcd58342a341
title: Pseudo-nodes need no link; state notes drawn; legend labels
truth:
---
## Problem

The renderer and the `nodes` check need a `term` or `noun` on every diagram node. A `start`, `end`, `choice` or `decision` node marks a place in a flow and names nothing, so a state spec cannot pass without a fake link. The review of issue 9 (`analysis/review-issue-9-m5.md`, L9) also found two silent drops. A `state` node's `note` is not drawn and gives no finding. A legend item with no `label` draws a swatch with no words.

## Shape

| Before | After |
|---|---|
| A node with no `term` and no `noun` refuses `nodes/resolve`. | A `start`, `end`, `choice` or `decision` node with neither passes. Every other node still refuses. A pseudo-node that gives a link is still resolved. Additive. |
| The `nodes/resolve` detail: "give exactly one of `term` … and `noun` …". | The detail adds: "only a `start`, `end`, `choice` or `decision` node may give neither". |
| A `state` or workflow `step` drops its `note` with no finding. | The note draws in muted lines under the name. A noted state draws as a rounded box (radius 22), so no corner cuts a line; a state with no note keeps its pill. A `start`, `end`, `choice` or `decision` node, or any `er` node, with a `note` or `sub` refuses `spec/schema`, and the finding names that key. No shipped `er` spec has a note. |
| A `legend` item with no `label`, or with no `role` or `style`, draws a bare swatch. | It refuses `spec/schema` on `legend`, and names the item by its index. Breaking for a spec that has such an item; no shipped spec has one. |
| `diagrams.md` and `workflow-model.md` say every node takes one link. `workflow-model.md` and `component.md` give a terminal state the role `end`. | They state the new rule. The workflow model keeps a states table whose first column is the state, so each state resolves as a `noun`. A terminal state is a named `state` with an edge to one `end` node. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram
~~~
