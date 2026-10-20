---
name: bp-deliberate
description: Use when a question comes up that the context cannot answer and the human must choose. Open a thread, compare the options, and record the human's choice.
---

Intent: you have a question that the slice and the vault cannot answer. The human makes the choice. You prepare it and record it.

1. Search first: `bp search <terms> --kind decision --json`, then `--kind thread`. A decision can already answer the question. Then use it, and open no thread. An open thread on the question is your thread: go to step 3.
2. Open a thread. Get the body from `bp create thread --template --json`, and replace each `TODO(bp):` line. Write the body to a file outside the vault. Then run `bp create thread --title "<question>" --project <project> --body-file <file> --json`. Link each design, decision or learning that the question rests on: `bp link <thread> cites <node> --json`.
3. Read the slice: `bp walk <thread> --for deliberate --json`. It holds the thread and the decisions and threads near it.
4. Find the options. A question can need sources outside the vault. Then dispatch the `bp-researcher` role with a brief. Write what you read, tried and compared into the thread: `bp set <thread> --body-file <file> --json`.
5. Give the human each option in a few lines, and recommend one. Do not choose for the human. The human can need time. Then pause the thread: `bp transition <thread> paused --json`.
6. Record the human's choice as a decision. Get the body from `bp create decision --template --json`. Then run `bp create decision --title "<the choice>" --project <project> --body-file <file> --json`. Run `bp link <decision> answers <thread> --json`. Run `bp transition <decision> accepted --json`.
7. The answer can be a lesson, not a choice. Then run the `bp-distil` skill with the thread as its source.
8. Close the thread with the human's words: `bp transition <thread> closed --reason "<the human's words>" --json`.

Stop: `bp walk <thread> --for deliberate --done --json` returns `done` true.
