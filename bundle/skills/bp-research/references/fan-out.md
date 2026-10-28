# Fan-out

Read this reference when the question has more than one part or asks for a judgement. You are the lead.

1. **Split.** Split the question into parts that one source each can settle. For a judgement, write two opposing briefs: two points of view, not two reads of one page.
2. **Dispatch.** With the `Agent` tool, call it once per part, at most three, all in one message, with `subagent_type: bp-researcher`. The prompt is your five dispatch lines, with `Skill: bp-research`, then a brief:
   - `Objective:` the part to settle.
   - `Questions:` the claims of the part, one line each.
   - `Sources:` the paths or nodes to prefer.
   - `Constraints:` the commit, and the bound of step 3 in its words.
   - `Return:` the `searched:` lines and the row shape of `## Findings`.

   Settle a part yourself only when the tool is absent, never because the part looks small. Dispatch gives each part a fresh context, and step 4 needs rows that you did not write.
3. **Bound.** A sub-researcher stops when each of its questions has a source. It also stops when a search returns nothing new, or at 15 tool calls. It does not dispatch: the harness allows one level, and the agent hook denies every other type.
4. **Challenge.** This is your own work after the dispatch. Each returned row is a claim: re-read or re-run its source before it enters your report, or mark it `unchecked`. Your `searched:` lines list the searches of the children that you repeated. When a part rests on one source, look for the source that contradicts it before you write `holds`.
