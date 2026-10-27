// The Milestone page (18, Milestone) and the Design page (18, Design) on their page objects: the Design page reads
// the staged copy's `projects/bp/designs/system` as `bp-view` served it.
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import system from "./fixtures/design-page.json";
import { DesignView } from "./pages/Design";
import { MilestoneView } from "./pages/Milestone";
import type { Design } from "./types/Design";
import type { Milestone } from "./types/Milestone";

afterEach(() => (cleanup(), vi.unstubAllGlobals(), window.history.pushState(null, "", "/")));

const id = "projects/bp/milestones/m";
const base = { id, kind: "milestone", record: false, hash: "h", lead: "", deltas: [], concerns: [], issues: [] };

const strip = () =>
  Array.from(document.querySelectorAll(".gates .g")).map((g) => [g.querySelector(".k")!.textContent, g.lastElementChild!.textContent]);

test("a planned milestone with no records: no blocks, rows not in vault, and the wait in words, not a stamp", () => {
  const m: Milestone = {
    ...base,
    title: "Staged planned milestone",
    status: "planned",
    intent: "needs-milestone-approval",
    note: `planned: the human approves it first: bp transition ${id} approved, then bp stamp ${id}`,
    finish_line: [],
    rows_not_in_vault: ["run", "delta", "concern-answer", "waiver"],
  };
  render(<MilestoneView m={m} project="bp" />);
  expect(strip()).toEqual([
    ["Finish line", "no blocks"],
    ["Carried deltas", "not in vault"],
    ["Concerns", "not in vault"],
    ["Issues", "no issues"],
  ]);
  expect(document.querySelector(".land .cmd")).toBeNull();
  expect(document.querySelector(".land .wait")?.textContent).toBe(m.note);
});

test("a milestone that needs acceptance: each block's last run, the waiver asked again, and the stamp to copy", () => {
  const m: Milestone = {
    ...base,
    title: "M",
    status: "approved",
    intent: "needs-acceptance",
    note: `bp stamp ${id}`,
    command: `bp stamp ${id}`,
    finish_line: [
      { name: "a", kind: "shell", severity: "error", last_run: { id: "r1", verdict: "pass", at: "2026-10-27T10:00:00Z", head: "1a2b3c4d5e" } },
      { name: "b", kind: "scenario", severity: "warn" },
    ],
    concerns: [{ key: "b", answer: "flaky host", state: "re-asked" }],
    issues: [{ id: "projects/bp/issues/0001.x", kind: "issue", title: "X", status: "resolved" }],
    rows_not_in_vault: ["delta", "concern-answer"],
  };
  render(<MilestoneView m={m} project="bp" />);
  expect(strip()).toEqual([
    ["Finish line", "1 of 2 blocks pass"],
    ["Carried deltas", "not in vault"],
    ["Concerns", "1 asked again"],
    ["Issues", "1 of 1 issue settled"],
  ]);
  const rows = Array.from(document.querySelectorAll(".checklist tr")).map((r) => Array.from(r.children).map((c) => c.textContent));
  expect(rows).toEqual([
    ["a", "check shell · error", "pass at 1a2b3c4 on 2026-10-27"],
    ["b", "check scenario · warn", "not run"],
  ]);
  expect(screen.getByRole("link", { name: "0001.x" }).getAttribute("href")).toBe("/bp/change/projects/bp/issues/0001.x");
  const land = document.querySelector(".land") as HTMLElement;
  expect(land.dataset.command).toBe(`bp stamp ${id}`);
  expect(document.activeElement).toBe(land);
});

const SVG = '<svg class="bp" viewBox="0 0 10 10"><g class="node" data-id="a"><rect></rect></g></svg>';

test("the system design: version banner, facts, outline, and each diagram after the section that cites it", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(SVG, { status: 200 })));
  render(<DesignView d={system as Design} project="bp" />);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Anvil — system design");
  expect(Array.from(screen.getByRole("note").children).map((s) => s.textContent)).toEqual([
    "no version",
    "status active",
    "deltas: not in vault",
  ]);
  expect(document.querySelector(".facts-line")?.textContent).toBe("level systemunder product3 diagramsbody 10 sections");
  const outline = screen.getByRole("navigation", { name: "outline" });
  expect(within(outline).getAllByRole("link")).toHaveLength(10);
  const fold = document.querySelector("details.fold") as HTMLDetailsElement;
  expect(fold.open).toBe(false);
  // The strip's link opens the fold at the figure; the body's own citation jumps there too.
  const links = screen.getAllByRole("link", { name: /anvil-runtime-flow/ });
  expect(links.map((a) => a.getAttribute("href"))).toEqual(["#fig-anvil-runtime-flow", "#fig-anvil-runtime-flow"]);
  fireEvent.click(links[0]);
  expect(fold.open).toBe(true);
  expect(within(outline).getByRole("link", { name: "Runtime flow" }).getAttribute("aria-current")).toBe("location");
  await waitFor(() => expect(document.querySelectorAll("figure.fig svg")).toHaveLength(3));
  const section = (name: string) => {
    let el = document.getElementById(`fig-${name}`)!.previousElementSibling;
    while (el && !/^H[23]$/.test(el.tagName)) el = el.previousElementSibling;
    return el?.id;
  };
  expect(["anvil-context", "anvil-two-loop", "anvil-runtime-flow"].map(section)).toEqual([
    "context-and-scope",
    "context-and-scope",
    "runtime-flow",
  ]);
  const urls = vi.mocked(fetch).mock.calls.map((c) => String(c[0]));
  expect(urls.sort()).toEqual(
    ["anvil-context", "anvil-runtime-flow", "anvil-two-loop"].map((n) => `/api/bp/design/projects/bp/designs/system/diagram/${n}`),
  );
});
