<!--
Instructions for the author. This comment does not show in the rendered pull request.

A human reviews this pull request without other context. Write so that they need nothing else.

- Write in ASD-STE100 Simplified Technical English. Keep domain terms.
  - A sentence holds at most 20 words for an instruction and 25 for a description.
  - Use the active voice. Put one idea in one sentence. Put the conclusion first.
  - Use one term for one concept, and use it the same way each time.
  - Do not use filler, hedges or marketing words.
- Make everything explicit.
  - Name each command, file, verb, code, flag and number.
  - Do not write "fixes the bug", "improves", "cleans up" or "as discussed". Say what was wrong and what is true now.
  - Do not refer to a conversation, a session or a review that the reader cannot open. Link or quote it.
- Fill every section. If a section does not apply, write "None" and one sentence that says why.
- If you cannot say who needs this change, do not open the pull request.
- Delete this comment.
-->

## Problem

<!-- What is wrong or missing today? Give the evidence: a command and its output, or a measurement. Say what you observed and what you expected. -->

## Why it needs solving now

<!-- What fails, or what stays blocked, if this pull request does not merge? Name the issue, milestone or design that requires it. -->

## Who needs it

<!-- Who meets the problem: which role (human, agent, implementer, reviewer) or which verb's caller, and in which situation? Does that person or agent need this now? Say why, with evidence. If no one needs it now, say so. -->

## What changed

<!-- The behaviour before and after, one row each. Name each verb, flag, refusal code, output field and file that changed. Mark each change to the contract (codes, flags, fields, exit codes) as additive or breaking. -->

| Before | After |
|---|---|
| | |

## How it works

<!-- The approach, in the order the code runs it. Name the crates and modules. Name each alternative you rejected and give the reason. -->

## Decisions for the reviewer

<!-- Each choice this pull request makes that no design or issue states. For each one: the choice, the reason, and what changes if the reviewer rejects it. -->

## Evidence

<!-- Paste the results; do not summarise them. Give the gate (fmt, clippy, deny, machete, deps, tests with their count), each check block by name with PASS, FAIL or WARN, and each measurement with its bound. Give the command that reproduces each result. -->

## Design changes

<!-- Each design document this pull request changes or depends on, with the commit in the designs repository. Write "None" if the designs already state everything. -->

## Out of scope and risks

<!-- What this pull request does not do, and where that work goes. What could break, and how a reviewer can see it. How to revert. -->
