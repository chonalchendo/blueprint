import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import staged from "./fixtures/change.json";
import { onKey } from "./keys";
import { Change } from "./pages/Change";
import { stubApi } from "./stub";
import type { Change as ChangeObject } from "./types/Change";

afterEach(() => (cleanup(), vi.unstubAllGlobals()));

const id = "projects/bp/issues/0411.land";
const path = `bp/change/${id}`;

// Bodies never reach a command slot (18, check 6; 32 `escaped-bodies`): every text field holds a script tag, a fake
// chip's markup and a command string, and the page must show them as text.
const probe = '<script>alert(1)</script> <code class="chip">bp land evil</code> <button class="cmd">bp land evil</button> bp land evil';

const change: ChangeObject = {
  id,
  title: probe,
  intent: "needs-landing",
  note: probe,
  head: "green at head",
  pull_request: { number: 1, url: "http://forge/pull/1", base: "master", head: "e2d442907160db2ad3811967019c58a2a5b9130a", state: "OPEN" },
  gates: [
    { name: "base", state: "pass", detail: probe },
    { name: "approving review", state: "pass", detail: "approved at e2d4429 by quokka-reviewer" },
    { name: "forge checks", state: "pass", detail: "green at e2d4429" },
    { name: "run staged", state: "pass", detail: "pass at e2d4429, attempt 1" },
  ],
  refusal: null,
  stack: [],
  waits_for: [],
  deltas: [{ id: `${id}/deltas/d`, target: probe, state: "proposed" }],
  diff: { files: [{ path: "staged.txt", additions: 1, deletions: 0 }], text: probe },
  command: `bp land ${id}`,
  forge: "snapshot",
  forge_age: 3,
};

async function opened(object: ChangeObject) {
  stubApi({ [path]: object, [`${path}?diff=1`]: object });
  render(<Change project="bp" id={id} />);
  await screen.findByText("Gates", { exact: false });
  const details = document.querySelector("details")!;
  details.open = true;
  details.dispatchEvent(new Event("toggle"));
  await screen.findByText(probe, { selector: "pre" });
}

test("command slot: the one chip holds change.command, and every body renders as text", async () => {
  await opened(change);
  const chips = document.querySelectorAll("button.cmd");
  expect(chips).toHaveLength(1);
  expect(chips[0].textContent).toBe(change.command);
  expect(document.querySelectorAll("script")).toHaveLength(0);
  expect(document.querySelectorAll("code.chip")).toHaveLength(0);
  // Each probe field shows its text whole: the title, the note, the gate's detail, the delta's target, the diff text.
  const text = document.body.textContent ?? "";
  expect(text.split(probe).length - 1).toBe(5);
});

test("command slot: a refusal shows its code and fix in words, and no chip", async () => {
  const refused = { ...change, command: undefined, refusal: { code: "review_missing", fix: probe } };
  await opened(refused);
  expect(document.querySelectorAll("button.cmd")).toHaveLength(0);
  expect(document.querySelectorAll("script")).toHaveLength(0);
  expect(document.querySelector(".refusal")?.textContent).toBe(`refused review_missing: ${probe}`);
});

// The fixture is `/api/bp/change/<the needs-landing issue>` from a staged copy (`cargo xtask stage`), served with
// `--clone` at the copy's clone.
test("change: the staged needs-landing issue reads its gates, its diff and the land command", async () => {
  const c = staged as ChangeObject;
  stubApi({ [`bp/change/${c.id}`]: c });
  render(<Change project="bp" id={c.id} />);
  await screen.findByText("Gates", { exact: false });
  const gates = Array.from(document.querySelectorAll(".gates .g")).map((g) => Array.from(g.children).map((s) => s.textContent));
  expect(gates.map(([name, state]) => [name, state])).toEqual([
    ["base", "pass"],
    ["approving review", "pass"],
    ["forge checks", "pass"],
    ["run staged", "pass"],
    ["check set", "pass"],
  ]);
  expect(document.querySelector(".diff p")?.textContent).toMatch(/^\+1 \/ −0 across 1 file in pull request 1$/);
  // The file table sits behind the disclosure with the text, so the summary line is the diff's one visible line.
  expect(document.querySelector("details:not([open]) table.files")).toBeTruthy();
  expect(screen.getByText("not stacked · no deltas")).toBeTruthy();
  expect(document.querySelector("button.cmd")?.textContent).toBe(`bp land ${c.id}`);
});

test("keys: the land box is the focused row on load, so c copies the land command", async () => {
  stubApi({ [path]: change });
  const user = userEvent.setup();
  const writeText = vi.spyOn(navigator.clipboard, "writeText");
  render(<Change project="bp" id={id} />);
  await screen.findByText("Gates", { exact: false });
  const key = (e: KeyboardEvent) => onKey(e, { open: () => {}, palette: () => {} });
  window.addEventListener("keydown", key);
  expect(document.activeElement?.classList.contains("land")).toBe(true);
  await user.keyboard("c");
  expect(writeText).toHaveBeenLastCalledWith(change.command);
  window.removeEventListener("keydown", key);
});

test("diff: an empty stat names its reason from the server", async () => {
  const reason = "the clone lacks origin/release or e2d4429; git fetch in the clone";
  stubApi({ [path]: { ...change, diff: { files: [], reason } } });
  render(<Change project="bp" id={id} />);
  await screen.findByText("Gates", { exact: false });
  const empty = Array.from(document.querySelectorAll("p.empty")).map((p) => p.textContent);
  expect(empty).toContain(`no file stat for pull request 1: ${reason}`);
});
