import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Moving } from "./pages/Moving";
import { stubApi } from "./stub";
import type { Moving as MovingObject } from "./types/Moving";
import type { MovingRow } from "./types/MovingRow";

afterEach(() => (cleanup(), vi.unstubAllGlobals()));

const issue = (slug: string, more: Partial<MovingRow>): MovingRow => ({
  id: `projects/bp/issues/${slug}`,
  kind: "issue",
  title: slug,
  intent: "working",
  note: "",
  head: "no pull request",
  ...more,
});

const moving: MovingObject = {
  project: "bp",
  forge: "snapshot",
  forge_age: 4,
  groups: [
    {
      group: "needs-landing",
      label: "needs landing",
      intents: ["needs-landing"],
      rows: [
        issue("0411.land", { intent: "needs-landing", holder: "worker", head: "green at head", command: "bp land projects/bp/issues/0411.land" }),
      ],
    },
    {
      group: "needs-acceptance",
      label: "needs acceptance",
      intents: ["needs-acceptance", "needs-milestone-approval", "re-asked"],
      rows: [
        {
          id: "projects/bp/milestones/m",
          kind: "milestone",
          title: "M",
          intent: "needs-milestone-approval",
          note: "",
          command: "bp transition projects/bp/milestones/m approved",
        },
      ],
    },
    {
      group: "blocked",
      label: "blocked",
      intents: ["blocked", "drifted"],
      rows: [issue("0412.esc", { intent: "blocked", note: "escalated", wait: "blocked: escalated" })],
    },
    {
      group: "ready",
      label: "ready",
      intents: ["ready"],
      rows: [
        issue("0413.a", { intent: "ready", command: "bp cut projects/bp/issues/0413.a" }),
        issue("0414.b", { intent: "ready", command: "bp cut projects/bp/issues/0414.b" }),
      ],
    },
    {
      group: "working",
      label: "working",
      intents: ["working", "waiting", "needs-review", "needs-response", "needs-approval", "needs-derivation", "needs-shaping"],
      rows: [
        issue("0415.w", { holder: "worker", wait: "held by worker since 2026-10-27" }),
        issue("0416.s", { holder: "worker", note: "stale", command: "bp release projects/bp/issues/0416.s --reason lost" }),
        issue("0417.r", { intent: "needs-review", holder: "worker", head: "checks pending", wait: "review dispatch" }),
      ],
    },
  ],
};

test("moving: five groups in order with their counts, and one chip per command row", async () => {
  stubApi({ "bp/moving": moving });
  render(<Moving project="bp" />);
  await screen.findAllByRole("row");
  const headers = Array.from(document.querySelectorAll("h2.group")).map((h) => h.textContent);
  expect(headers).toEqual(["needs landing1", "needs acceptance1", "blocked1", "ready2", "working3"]);
  expect(document.querySelector("p.sub")?.textContent).toBe("2 decisions due · 6 more moving · forge read 4 s ago");
  const rows = screen.getAllByRole("row");
  expect(rows).toHaveLength(8);
  for (const row of rows) {
    const chips = within(row).queryAllByRole("button");
    expect(chips.length).toBe(row.dataset.command ? 1 : 0);
    if (row.dataset.command) expect(chips[0].textContent).toBe(row.dataset.command);
    else expect(row.querySelector(".wait")?.textContent).toBeTruthy();
  }
  expect(screen.getAllByRole("button")).toHaveLength(5);
  // The first decision is the focal row.
  expect(document.querySelectorAll("tr.focal")).toHaveLength(1);
  expect(document.querySelector("tr.focal")?.getAttribute("data-command")).toBe("bp land projects/bp/issues/0411.land");
  const land = rows[0];
  expect(Array.from(land.querySelectorAll("td")).map((c) => c.textContent)).toEqual([
    "0411.landissue",
    "worker",
    "0411.landneeds landing · green at head",
    "bp land projects/bp/issues/0411.land",
  ]);
  expect(land.dataset.href).toBe("/node/projects/bp/issues/0411.land");
  expect(rows[2].querySelector("td.holder")?.textContent).toBe("—");
});

test("moving: an empty group says nothing, and a zero keeps its noun", async () => {
  const empty = { ...moving, forge: "not read" as const, forge_age: undefined, groups: moving.groups.map((g) => ({ ...g, rows: [] })) };
  stubApi({ "bp/moving": empty });
  render(<Moving project="bp" />);
  await screen.findAllByText("nothing");
  expect(Array.from(document.querySelectorAll("p.empty")).map((p) => p.textContent)).toEqual(Array(5).fill("nothing"));
  expect(document.querySelector("p.sub")?.textContent).toBe("no decisions due · nothing more moving · forge not read");
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});
