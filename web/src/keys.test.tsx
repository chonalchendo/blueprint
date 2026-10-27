import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { App } from "./App";
import { Command } from "./Command";
import { onKey } from "./keys";
import { Row } from "./Row";
import { stubApi } from "./stub";

const projects = {
  rows: [
    { id: "projects/bp/project", kind: "project", title: "Blueprint", intents: {} },
    { id: "projects/demo/project", kind: "project", title: "demo", intents: {} },
  ],
};

beforeEach(() => window.history.replaceState(null, "", "/"));
afterEach(() => (cleanup(), vi.unstubAllGlobals()));

test("j and k move the focused row, and enter opens it", async () => {
  stubApi({ projects, bp: { id: "projects/bp/project", nodes: {}, records: {}, designs: [], conventions: [], milestones: [], intents: {} } });
  const user = userEvent.setup();
  render(<App />);
  const rows = await screen.findAllByRole("row");
  await user.keyboard("j");
  expect(document.activeElement).toBe(rows[1]);
  await user.keyboard("jjj");
  expect(document.activeElement).toBe(rows[2]);
  await user.keyboard("k");
  expect(document.activeElement).toBe(rows[1]);
  await user.keyboard("{Enter}");
  expect(window.location.pathname).toBe("/bp");
  expect((await screen.findAllByText("nothing")).length).toBeGreaterThan(0);
});

test("c copies the focused row's command; the chip copies on click", async () => {
  // user-event puts its own clipboard on the navigator; the spy reads what the view wrote to it.
  const user = userEvent.setup();
  const writeText = vi.spyOn(navigator.clipboard, "writeText");
  render(
    <table>
      <tbody>
        <Row href="/bp/change/0411" command="bp land 0411">
          <td>
            <Command text="bp land 0411" />
          </td>
        </Row>
        <Row href="/bp/change/0412">
          <td>no command</td>
        </Row>
      </tbody>
    </table>,
  );
  const key = (e: KeyboardEvent) => onKey(e, { open: () => {}, palette: () => {} });
  window.addEventListener("keydown", key);
  await user.keyboard("j");
  await user.keyboard("c");
  expect(writeText).toHaveBeenLastCalledWith("bp land 0411");
  await user.keyboard("jc");
  expect(writeText).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "bp land 0411" }));
  expect(writeText).toHaveBeenCalledTimes(2);
  window.removeEventListener("keydown", key);
});

test("/ opens the palette; enter jumps to the chosen hit's node", async () => {
  stubApi({
    projects,
    "search?q=vault": { rows: [{ id: "projects/bp/designs/system", kind: "design", title: "System", rank: 1, snippet: "the «vault»" }] },
    "bp/design/projects/bp/designs/system": {
      id: "projects/bp/designs/system", kind: "design", title: "System", status: "active", record: false, hash: "h", lead: "",
      body: "b", level: "system", diagrams: [], deltas: [], version: "no version", outline: [], rows_not_in_vault: ["delta"],
    },
  });
  const user = userEvent.setup();
  render(<App />);
  await screen.findAllByRole("row");
  await user.keyboard("/");
  const input = screen.getByRole("textbox", { name: "search or id" });
  await user.type(input, "vault");
  await screen.findByRole("option");
  expect(screen.getByRole("option").textContent).toContain("projects/bp/designs/system");
  await user.keyboard("{Enter}");
  expect(window.location.pathname).toBe("/bp/design/projects/bp/designs/system");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(await screen.findByRole("heading", { level: 1, name: "System" })).toBeTruthy();
});

test("⌘K opens the palette, an id typed in full jumps without a hit, and escape closes it", async () => {
  stubApi({ projects });
  const user = userEvent.setup();
  render(<App />);
  await screen.findAllByRole("row");
  await user.keyboard("{Meta>}k{/Meta}");
  await user.type(screen.getByRole("textbox"), "projects/bp/issues/0411.x");
  expect(screen.getByRole("option").textContent).toContain("open by id");
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  await user.keyboard("/");
  await user.type(screen.getByRole("textbox"), "projects/bp/issues/0411.x");
  await act(async () => user.keyboard("{Enter}"));
  expect(window.location.pathname).toBe("/node/projects/bp/issues/0411.x");
});
