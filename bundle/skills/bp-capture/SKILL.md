---
name: bp-capture
description: Use when the human parks a thought for later and wants no action on it now. Write one inbox entry.
---

Intent: the human parks a thought. Keep it, and do no work on it.

On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.

1. Write the thought to a file outside the vault. Use one or two sentences. Say what the human saw, and where.
2. Create the entry: `bp create inbox --title "<one line>" --body-file <file> --json`. Add `--project <project>` when the thought is about one project.
3. Give the human the `id` of the entry. Then go back to the earlier task.

Do not link, promote or act on the entry. A later session places it.

Stop: `bp walk <id> --for capture --done --json` returns `done` true. Here `<id>` is the entry.
