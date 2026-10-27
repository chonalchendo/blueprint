// 18, check 2 and 32 `every-node-serves` (the client half): the node page renders one `bp show --json` object of every
// node and record kind in the converted vault, taken from it with `bp --json show <id>`, less the
// `contract_version` key that `/api/node/<id>` strips, so the test renders what the client receives.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { header } from "./header";
import { NodeView } from "./pages/Node";
import type { Show } from "./types/Show";

const fixtures = import.meta.glob<Show>("./fixtures/*.json", { eager: true, import: "default" });

// The kinds `bp list --count-only` counts in the converted vault: nine node kinds and four record kinds.
const KINDS = ["convention", "decision", "design", "inbox", "issue", "learning", "milestone", "project", "thread"];
const RECORDS = ["claim", "handoff", "review", "run"];

afterEach(cleanup);

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
    if (show.body) expect(document.querySelector("pre.body")?.textContent).toBe(show.body);
    unmount();
  }
});

test("a body renders as text, never as markup", () => {
  const show = { id: "projects/bp/issues/x", kind: "issue", record: false, hash: "h", lead: "", body: "<script>alert(1)</script><b>x</b>" } as Show;
  render(<NodeView show={show} />);
  expect(document.querySelector("script")).toBeNull();
  expect(document.querySelector("pre.body b")).toBeNull();
  expect(document.querySelector("pre.body")?.textContent).toBe(show.body);
});

test("the header narrows kind, title and status, and refuses an object without a kind", () => {
  const show = { id: "a", kind: "issue", title: "T", status: 3, record: false, hash: "h", lead: "" } as unknown as Show;
  expect(header(show)).toEqual({ id: "a", kind: "issue", title: "T", status: undefined });
  expect(() => header({ id: "a" } as unknown as Show)).toThrow(/not a show object/);
});
