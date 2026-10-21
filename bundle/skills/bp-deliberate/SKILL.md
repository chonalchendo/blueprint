---
name: bp-deliberate
description: Use when a question comes up that the context cannot answer and the human must choose. Open a thread, compare the options, and record the human's choice.
---

Intent: you have a question that the slice and the vault cannot answer. Only the human chooses the answer. You prepare the choice, and you record it.

On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.

1. Look for an answer: `bp search <terms> --kind decision --json`, then `--kind thread`. A decision can already answer the question. Then use it, and open no thread. An open thread on the question is your thread. Then go to step 3.
2. Open a thread. Get the body from `bp create thread --template --json`. Write the question and what you read so far. Write "Nothing yet." in each slot that you cannot fill now. Leave no `TODO(bp):` line. Save the body to a file outside the vault. Then run `bp create thread --title "<question>" --project <project> --body-file <file> --json`. Link each design, decision or learning that the question rests on: `bp link <thread> cites <node> --json`.
3. Read the slice: `bp walk <thread> --for deliberate --json`. It holds the thread and the decisions and threads near it.
4. Find the options. For sources outside the vault, dispatch the `bp-researcher` role with a brief. Put what you read, tried and compared into the thread: `bp set <thread> --body-file <file> --json`.
5. Give the human each option in a few lines. Recommend one option. Do not choose for the human. The human can need time. Then pause the thread: `bp transition <thread> paused --json`.
6. When the answer is a choice, record it as a decision. Get the body from `bp create decision --template --json`. Run `bp create decision --title "<the choice>" --project <project> --body-file <file> --json`. Run `bp link <decision> answers <thread> --json`. Run `bp transition <decision> accepted --json`.
7. Close the thread with the human's words: `bp transition <thread> closed --reason "<the human's words>" --json`.
8. When the answer is a lesson, run the `bp-distil` skill after step 7. Its source is the closed thread.

Stop: `bp walk <thread> --for deliberate --done --json` returns `done` true.
