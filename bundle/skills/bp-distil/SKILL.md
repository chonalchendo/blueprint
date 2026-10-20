---
name: bp-distil
description: Use when a landed issue, a closed thread or a run taught a reusable lesson. Write one learning with a cue, and record evidence on the learnings that the source applied.
---

Intent: a source taught a lesson that other work can reuse. The source is a landed issue, a closed thread, or the node that a run sits under. This text calls it `<source>`.

1. Read the slice: `bp walk <source> --for distil --json`. It holds the source and the learnings near it.
2. Search for the lesson: `bp search <terms> --kind learning --json`. A learning can already state it. Then use that learning, and write no second one. Go to step 5.
3. Write one claim in one sentence to a file outside the vault. Put no evidence in it. Then run `bp create learning --title "<the claim>" --body-file <file> --json`. A learning is shared, so pass no `--project`.
4. Set the cue: `bp set <learning> cue "<one line>" --json`. The cue holds the words that a later search for this lesson uses.
5. Cite the source. For an issue or a design, run `bp link <learning> cites <source> --json`. A learning cannot cite a thread. So for a thread, add `[[<source>]]` as the last line of the body: `bp set <learning> --body-file <file> --json`.
6. Record evidence. The slice marks each learning with `stale`: true when a design it cites changed after its last row. Record one row for each learning that the source applied or tested. Use `bp record evidence <learning> --field outcome=<outcome> --field issue=<issue> --field note="<one line>" --json`. The outcome is `helped`, `hurt`, `no-effect` or `confounded`. Omit `issue` when the source is not an issue. A stale learning can no longer hold. Then record `hurt`, and run `bp transition <learning> retracted --json`.
7. Leave a new learning in `draft`.

Stop: `bp walk <source> --for distil --done --json` returns `done` true.
