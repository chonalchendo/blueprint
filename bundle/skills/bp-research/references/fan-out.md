# Fan-out

Read this when the question has parts or asks for a judgement. You are the lead.

1. **Split.** Split the question into parts that one source each can settle. For a judgement, write two opposing briefs: two points of view, not two reads of one page.
2. **Dispatch.** With the `Agent` tool, call it once per part, at most three, all in one message, with `subagent_type: bp-researcher`. The prompt is your five dispatch lines, unchanged, `Budget:` too, then a brief:
   - `Objective:` the part to settle.
   - `Questions:` the claims of the part, one line each.
   - `Sources:` the paths or nodes to prefer.
   - `Constraints:` the commit, then this line, copied: "Stop when each question has a source. Stop when a search returns nothing new. Stop at 15 tool calls. Do not dispatch."
   - `Return:` the `searched:` lines and the row shape of `## Findings`.

   Settle a part yourself only when the tool is absent, never because the part looks small. Dispatch gives each part a fresh context, and step 4 needs rows you did not write.
3. **Bound.** A sub-researcher stops by the line in `Constraints:`. The harness allows one level, and the agent hook denies every other type.
4. **Challenge.** After the dispatch, test each returned row yourself. Re-run its command, re-read its path, or open its node with `bp show <id> --json`. A search hit is not a read. A row that you did not open or re-run is `unchecked`. Your `searched:` lines list only the searches that you ran, never a child's. When a part rests on one source, look for the source that contradicts it before you write `holds`.
