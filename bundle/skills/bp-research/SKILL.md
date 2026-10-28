---
name: bp-research
description: Use when a researcher dispatch asks one question. Answer it with claims you tested, each with its source, and say when the vault's earlier answer is wrong.
---

Task: answer the question below the five lines of the dispatch. This text calls the node on the `Node:` line `<id>`.

1. Before you read anything, write the claim list from the question: each quantity, name and rule that the answer needs, one line each.
2. Read the slice: `bp walk <id> --for research --json`. Search the vault with `bp search <terms> --json`: two or three distinctive nouns, in the source's own word forms. Then widen one term at a time. The search needs every term and has no stemming, so `token` does not find `tokens`. A search result is not a read. Open the node with `bp show <node> --json`, or open the file, before you cite it.
3. The vault's earlier answer is a claim to test, not the answer, when its source is in reach. A learning, decision or design can cite a path, a command or a measurement. Re-read that path or re-run that command in the clone.
4. Test each claim. Look for the source that contradicts it. When a command settles the claim, run it. Run a command that writes files in a temporary directory, not in the clone. A claim about the harness is `unchecked` until a command or a session shows it. Its row names the probe that would settle it. Keep the qualifier of the source verbatim, such as "under 50%" or "on one model".
5. Keep to the question. Write a new question as one line under `## Open`, and do not research it.

Read `references/fan-out.md` only when the question has more than one part or asks for a judgement. A brief with an `Objective:` line comes from a lead: then do not read it.

Report in this shape:

- `## Answer`: one paragraph.
- `## Findings`: first the line `at: <sha>`, the clone commit that each `<path>:<line>` refers to. Then the table `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`, its separator line, and one row per claim, at most 20 rows. The source is `<path>:<line>`, a backticked command, a node id or a URL. A row whose source you did not open is `unchecked`, whatever it says. Mark a row `measured` when a command you ran shows the claim. Mark it `documented` when a source you opened states it, and `inferred` when you reasoned it.
- `## Open`: one line per open question. An absence claim ("no source shows X") carries `searched:` and the places you looked. A `not run:` line names a command that the tools denied.

Stop: return the report. Research is call-shaped, so `bp walk <id> --for research --done --json` returns `done` true when you return it.
