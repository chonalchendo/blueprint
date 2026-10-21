# Revision

Load this reference when the design is past `draft`. Load it before the reference of the changed level. It produces one delta on the design, with the whole proposed body and each changed diagram spec. The change text names the class and the cause. The delta changes the design only when it is accepted.

## Procedure

1. **State the change**. Write one sentence for the change. Name the target design and the cause: an issue, a drift run, a hotspot or a thread. The human confirms the sentence.
2. **Read the baseline**. Read the design: `bp show <design> --json`. Read its last accepted text with `bp show <design> --at <tag> --json`. The tag is `blueprint/<p>/<n>`. List the versions with `bp list blueprint-version --project <p> --json`. List the deltas since: `bp list delta --under <design> --json`.
3. **Find the impact by walk**. Run `bp walk <design> --for design --ids --json`. Read the header of each id it reaches with `bp show <id> --no-body --json`. Sort each id by its link to the design.
   - A child design `under` it, and each view attached to it.
   - A design, issue or decision that `cites` it.
   - A concern answer that names it.
   - A decision that `authorizes` the change, or `supersedes` a decision of the changed section.
   - Each check block in `## Checks`, and each diagram whose home is the design.
4. **Set the class**. Choose one class for the change:
   - **Clarifying**: the meaning stays. Wording, a label or a diagram cell changes.
   - **Additive**: a new noun, section, interface row or diagram node. Nothing that exists changes meaning.
   - **Breaking**: a noun, interface, permitted connection, characteristic or threshold changes meaning or goes away.
   Tell the human the class and the impact list. The human confirms the class.
5. **Choose the path**. A clarifying change can take the editorial path. Any other class needs a milestone. Find it with `bp list milestone --project <p> --status planned --json`. Use `--status approved` when none is planned. With no milestone, tell the human and stop. Recommend a thread or an editorial split.
6. **Re-check by class**. Run the gates of `references/gates.md` on the proposed design. Each class runs these gates:
   - Clarifying: G1, G7 and G9. On the editorial path, tell the human each gap that stays, and add no section or heading.
   - Additive: the clarifying gates, then G2. Add G3 for a new characteristic. Each new noun gets a glossary term.
   - Breaking: all twelve gates. Also read each sentence of each citing design that uses the changed term. Supersede each decision whose context names the changed fact: write a new decision, and never edit the old one. Run `bp check <id> --json` on the design and on each child. A renamed concept gets a new term, and the old term retires.
   - Each class: does the change contradict the parent design? If it does, the parent needs its own delta, or the change is wrong.
7. **Update the diagrams**. Do this for each diagram whose home is the design. Load `references/diagrams.md` first.
   - Copy the spec from `<vault>/<design>/diagrams/<name>.yaml` to a file outside the vault. `bp where --json` gives the vault.
   - Edit the copy. Keep each node id. Change the fewest lines.
   - Render the diff: `bp diagram --base <old-spec> --spec-file <new-spec> --out <page> --json`. Show the page to the human.
   - A changed context or container diagram runs risk storming again on the changed region. Use `references/risk-threat.md`.
   - A child design whose diagram shows a changed noun gets a follow-up line in the delta's change.
8. **Check the body diff**. Write the whole proposed body to a file outside the vault. Compare it with the current body. Each section that goes away needs the human's word. A net loss that the human did not ask for is a defect.
9. **Confirm and record**. Show the human the proposed body one changed section at a time. Then record one delta.
   - Milestone path: run `bp record delta <design> --field milestone=<milestone> --field cause=<id> --field change="<what changes>" --body-file <file> --json`.
   - Add `--field authorized_by=<decision>` when a decision authorizes the change.
   - Add `--diagram <name>=<spec>` for each changed or new diagram. Add `--remove-diagram <name>` for each dropped diagram.
   - Editorial path: add `--editorial` to `record delta` in place of the milestone field.
   - Run the call with `--dry-run` first. Fix each item of `findings` in the body file. Then run it without `--dry-run`.
   - When the change moves the answer to a concern, record the new answer. Run `bp record concern-answer <project> --field concern=<concern> --field state=<state> --field design=<design> --json`.
10. **Hand over**. Tell the human that the delta exists, its class and its impact list. A milestone delta is accepted when the human stamps the milestone. The human accepts an editorial delta with `bp stamp <delta> --json`.

## Questions for the human

- Does the change sentence say what a reader of the design will see differ?
- Is the class right? A breaking change that you call additive skips gates.
- Does each item of the impact list need a follow-up, or does it still read true?
- Does the diff render show only the changed elements?
