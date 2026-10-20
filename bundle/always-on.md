## Blueprint

- `bp` is the only writer of the vault. Do not edit a vault file by hand.
- Pass `--json` to each `bp` call. On a non-zero exit, read `code` and run `fix`.
- Start each task from `bp walk <id> --for <action> --json`.
