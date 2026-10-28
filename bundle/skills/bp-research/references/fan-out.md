# Fan-out

Read this reference when the question has more than one part or asks for a judgement. You are the lead.

1. **Split.** Split the question into parts that one source each can settle. When the question asks for a judgement, write two opposing briefs. Then you get two independent points of view, not two reads of one page.
2. **Dispatch.** With no `Agent` tool, settle each part yourself, in turn. Otherwise, dispatch one `bp-researcher` sub-researcher per part, at most three, all in one turn. Give each the five lines of your own dispatch, with `Skill: bp-research`. Below the five lines, write its brief in five labelled lines:
   - `Objective:` the part to settle.
   - `Questions:` the claims of the part, one line each.
   - `Sources:` the paths or nodes to prefer.
   - `Constraints:` the commit, and the bound of step 3 in its words.
   - `Return:` the `searched:` lines and the row shape of `## Findings`.
3. **Bound.** A sub-researcher does not read this reference, so its brief carries the bound. It stops when each of its questions has a source. It also stops when a search returns nothing new, or at 15 tool calls. It does not dispatch. The harness lets you dispatch one level, and the agent hook denies every type other than `bp-researcher`. So never ask a sub-researcher to dispatch.
4. **Challenge.** Each returned row is a claim. Re-run or re-read its source yourself before the row enters your report, or mark the row `unchecked`. When a part rests on one source, look for the source that contradicts it before you write `holds`.
