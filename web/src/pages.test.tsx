import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Home } from "./pages/Home";
import { Projects } from "./pages/Projects";
import { stubApi } from "./stub";
import type { Home as HomeObject } from "./types/Home";
import type { HomeDesign } from "./types/HomeDesign";
import type { Projects as ProjectsObject } from "./types/Projects";

afterEach(() => (cleanup(), vi.unstubAllGlobals()));

const projects: ProjectsObject = {
  rows: [
    { id: "projects/bp/project", kind: "project", title: "Blueprint", status: "active", version: "blueprint/bp/2", intents: { "needs-landing": 1, "needs-milestone-approval": 1, ready: 2, blocked: 1 } },
    { id: "projects/demo/project", kind: "project", title: "demo", status: "active", intents: {} },
  ],
};

test("projects: one row per project with its version, decisions due and items moving", async () => {
  stubApi({ projects });
  render(<Projects />);
  const rows = await screen.findAllByRole("row");
  expect(rows.slice(1).map((r) => Array.from(r.querySelectorAll("td")).map((c) => c.textContent))).toEqual([
    ["bp", "Blueprint", "blueprint/bp/2", "2", "3"],
    ["demo", "demo", "no version", "none", "nothing"],
  ]);
  expect(rows[1].dataset.href).toBe("/bp");
});

const design = (id: string, design_kind: string, more: Partial<HomeDesign> = {}): HomeDesign => ({
  id: `projects/bp/designs/${id}`,
  kind: "design",
  title: id,
  status: "active",
  design_kind,
  diagrams: 0,
  ...more,
});

const home: HomeObject = {
  id: "projects/bp/project",
  nodes: { design: 7, issue: 4, milestone: 3, project: 1 },
  records: {},
  designs: [
    design("product", "product"),
    design("system", "system", { diagrams: 3, under: "projects/bp/designs/product" }),
    design("containers/view", "container", { under: "projects/bp/designs/system" }),
    design("components/server", "component", { under: "projects/bp/designs/containers/view" }),
    design("components/pages", "component", { under: "projects/bp/designs/containers/view" }),
    design("components/walk", "component", { under: "projects/bp/designs/system" }),
    design("components/old", "component", { status: "retired" }),
    design("data-model", "data-model", { status: "draft" }),
  ],
  conventions: ["shared/conventions/convention.prose"],
  milestones: [
    { id: "projects/bp/milestones/the-human-view", kind: "milestone", title: "The human view", status: "planned", intent: "needs-milestone-approval", issues: { ready: 1, working: 1 } },
    { id: "projects/bp/milestones/old", kind: "milestone", title: "Old", status: "done", issues: { resolved: 5 } },
    { id: "projects/bp/milestones/dropped", kind: "milestone", title: "Dropped", status: "abandoned", issues: {} },
  ],
  intents: { "needs-landing": 1, "needs-milestone-approval": 1, ready: 1, working: 1 },
  forge: "not read",
};

test("project home: three panes, each a summary line with counts", async () => {
  stubApi({ bp: home });
  render(<Home project="bp" />);
  const panes = await screen.findAllByRole("group");
  const summaries = Array.from(document.querySelectorAll("details.pane > summary")).map((s) => s.textContent);
  expect(summaries).toEqual([
    "Blueprint7 designs in the tree · 1 retired or deprecated · coverage per concern: not in the vault",
    "Loop1 open milestone · 2 done or abandoned",
    "Moving2 decisions due · 2 more · forge not read",
  ]);
  expect(panes.length).toBeGreaterThanOrEqual(3);
});

test("blueprint pane: the tree folds components under their parent, and versions read not in vault", async () => {
  stubApi({ bp: home });
  render(<Home project="bp" />);
  const pane = (await screen.findByText("Blueprint")).closest("details")!;
  const folds = Array.from(pane.querySelectorAll("details.sub > summary")).map((s) => s.textContent);
  expect(folds).toEqual(["1 components", "2 components", "1 retired or deprecated"]);
  const system = within(pane).getByRole("link", { name: "system" }).closest("tr")!;
  expect(Array.from(system.querySelectorAll("td")).map((c) => c.textContent)).toEqual([
    "system", "system", "system", "active", "3 diagrams", "not in vault",
  ]);
  expect(pane.querySelector("p.views")?.textContent).toBe(
    "views beside the tree: data model data-model · glossary none · conventions convention.prose",
  );
});

test("loop and moving panes: open milestones first, decisions due by group, nothing for an empty group", async () => {
  stubApi({ bp: home });
  render(<Home project="bp" />);
  const loop = (await screen.findByText("Loop")).closest("details")!;
  const first = loop.querySelector(".in > table tr")!;
  expect(first.textContent).toBe("the-human-viewThe human viewplanned, needs milestone approval1 ready, 1 working");
  expect(loop.querySelector("details.sub > summary")?.textContent).toBe("2 done or abandoned");
  const moving = screen.getByText("Moving").closest("details")!;
  const groups = Array.from(moving.querySelectorAll("tr")).map((r) => r.firstChild?.textContent + " " + r.children[1].textContent);
  expect(groups).toEqual(["needs landing 1", "needs acceptance 1", "blocked nothing", "ready 1", "working 1"]);
  expect(within(moving).getByText("open Moving").getAttribute("href")).toBe("/bp/moving");
});

test("a refused page shows the refusal in words", async () => {
  stubApi({});
  render(<Home project="nope" />);
  expect((await screen.findByRole("alert")).textContent).toContain("no_node");
});
