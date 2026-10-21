---
name: bp-distil
description: Use when a landed issue, a closed thread or a run taught a reusable lesson. Write one learning with a cue, and record evidence on the learnings that the source applied.
---

Intent: a source taught a lesson that other work can reuse. The source is a landed issue, a closed thread, or the design, issue or milestone that a run sits under. This text calls it `<source>`.

On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.

1. Read the slice: `bp walk <source> --for distil --json`. It holds the source and the learnings near it. Each learning carries `stale`.
2. Look for the lesson: `bp search <terms> --kind learning --json`. A learning can already state it. Then use that learning, and write no new one. Go to step 4 if it has no `cue`, else go to step 5.
3. Write the lesson in one sentence to a file outside the vault. Put no evidence in it. For a thread or a milestone source, add `[[<source>]]` as the last line. Then run `bp create learning --title "<the lesson>" --body-file <file> --json`.
4. Set the cue: `bp set <learning> cue "<one line>" --json`. The cue holds the words that a later search for this lesson uses.
5. Cite the source. For an issue or a design, run `bp link <learning> cites <source> --json`. A learning from step 2 can need the `[[<source>]]` line of step 3. Then write it into the body: `bp set <learning> --body-file <file> --json`.
6. Re-check each learning in the slice whose `stale` is true. A design that it cites changed after its last evidence row, or after its creation when it has no row. Read that design, and test the lesson against it. When the lesson still holds, record a `rechecked` row in step 7. When it no longer holds, run `bp transition <learning> retracted --json`.
7. Record evidence. Record one row for each learning that the source applied, and for each learning that step 6 kept. Use `bp record evidence <learning> --field outcome=<outcome> --field issue=<issue> --field note="<one line>" --json`. The outcome is `helped`, `hurt`, `no-effect` or `confounded`. A row from step 6 is `rechecked`, and its note names the design change. It clears `stale`, and it adds no confidence. Omit `issue` when the source is not an issue.
8. Leave a new learning in `draft`. Only the human moves a learning to `verified`.

Stop: `bp walk <source> --for distil --done --json` returns `done` true.
