<!--
Instructions for the author. This comment does not show in the rendered pull request.

A human reviews this pull request without other context. Give them all the context they need, in the fewest words.

- Write in ASD-STE100 Simplified Technical English. Keep domain terms.
  - A sentence holds at most 20 words for an instruction and 25 for a description.
  - Use the active voice. Put one idea in one sentence. Put the conclusion first.
  - Use one term for one concept, and use it the same way each time.
- Be concise and explicit.
  - Write the fewest sentences that leave nothing implicit. Cut anything the reviewer does not need to judge the change.
  - Name each command, file, verb, code, flag and number. Do not write "fixes the bug", "improves" or "as discussed".
  - Link or quote each source. Do not refer to a conversation or a review that the reader cannot open.
- Decide what you can. Blueprint holds the context for most decisions. Put a decision under "Needs your decision" only when the owner must make it: scope or product direction, a breaking contract change, a new dependency, or a choice that no design settles and that is hard to reverse.
- Fill every section. If a section does not apply, write "None".
- If you cannot say who needs this change, do not open the pull request.
- Delete this comment.
-->

## Problem

<!-- What is wrong or missing today, with the evidence: a command and its output, or a measurement. -->

## Why now

<!-- What fails or stays blocked without this change. Name the issue, milestone or design that requires it. -->

## Who needs it

<!-- Which role or verb caller meets the problem, and why they need this now. -->

## What changed

<!-- One row per behaviour that changed. Mark each contract change (code, flag, field, exit code) as additive or breaking. -->

| Before | After |
|---|---|
| | |

## How it works

<!-- The approach in a few sentences. Name the crates and modules. For each choice you made, name the design that settles it, or give the reason in one sentence. -->

## Needs your decision

<!-- Only the choices the owner must make (see the instructions above). For each: the question, your recommendation, and what changes if the owner decides otherwise. Usually "None". -->

## Evidence

<!-- The results, not a summary: the gate with the test count, each check block by name with PASS, FAIL or WARN, each measurement with its bound, and a link to the review. -->

## Design changes and out of scope

<!-- The design documents this change updates, with the designs-repository commit. What this change does not do, and where that work goes. -->
