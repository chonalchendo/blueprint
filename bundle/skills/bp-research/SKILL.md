---
name: bp-research
description: Use when a researcher dispatch asks one question. Answer it with claims you tested, each with its source, and say when the vault's earlier answer is wrong.
---

Task: answer the question below the five lines of the dispatch. This text calls the node on the `Node:` line `<id>`.

1. Before you read anything, write the claim list. Give each quantity, name and rule that the answer needs, one line each. The `searched:` lines of your report hold the list. When the question has two or more parts, or asks for a judgement, read `references/fan-out.md` before step 2. When your brief has an `Objective:` line, a lead sent you: do not read `fan-out.md`.
2. Read the slice: `bp walk <id> --for research --json`. Then, before you read the clone, run one `bp search <terms> --json` for each line of the claim list. Use two or three distinctive nouns of the question, singular and plural. Every term must match, or any term when no row matches all. There is no stemming, so `token` does not find `tokens`. Widen one term at a time. Open each hit that the claim concerns with `bp show <node> --json`. A search result is not a read: open the node or the file before you cite it. A node that states the quantity, name or rule of a claim gets its own row. Its source is the node id. Its verdict is your test in the clone: `falsified` when the clone disagrees. The question can name the file, and a vault node can still state the quantity. A report that ignores that node is wrong, even when its number is right.
3. The vault's earlier answer is a claim to test, not the answer, when its source is in reach. Re-read the path that it cites, re-run its command, or measure its number again in the clone.
4. Test each claim. Look for the source that contradicts it. When a command settles the claim, run it. Run a command that writes files in a temporary directory, not in the clone. A claim about the harness is `unchecked` until a command or a session shows it. Its row names the probe that would settle it. Keep the qualifier of the source verbatim, such as "under 50%" or "on one model".
5. Keep to the question. Write a new question as one line under `## Open`, and do not research it.

Report in this shape:

- `## Answer`: one paragraph.
- `## Findings`: first the line `at: <sha>`, the clone commit that each `<path>:<line>` refers to. Then one line per line of the claim list: `searched: <terms>: <node ids the search returned, or none>`. Then the table, its separator line and one row per claim, at most 20 rows. The header is `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`. A source is `<path>:<line>`, a backticked command, a node id or a URL. A row whose source you did not open is `unchecked`, whatever it says. Mark a row `measured` when a command you ran in this session shows it. Mark it `documented` when only a source you opened states it, and `inferred` otherwise.
- `## Open`: one line per open question. An absence claim ("no source shows X") carries `searched:` and the places you looked. A `not run:` line names a command that the tools denied.

Stop: return the report as your last message, and make no tool call after it. Research is call-shaped: `bp walk <id> --for research --done --json` returns `done` true without your call.
