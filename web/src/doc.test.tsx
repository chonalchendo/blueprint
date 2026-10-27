// The document renderer (19, principle 9; 32, design change 8): a body is untrusted text, so nothing it holds runs;
// headings carry the outline's anchors; a check block reads under its name; each diagram is the renderer's SVG,
// inlined after the block that cites it, with the view's classes on hover and click.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Document } from "./doc/Document";
import { blocks, clean, outlineOf } from "./doc/markdown";
import large from "./fixtures/research-stage.md?raw";

afterEach(() => (cleanup(), vi.unstubAllGlobals(), vi.restoreAllMocks(), window.history.pushState(null, "", "/")));

test("a body's script, event handler and javascript: link never run: the sanitiser strips them", () => {
  const alert = vi.fn();
  vi.stubGlobal("alert", alert);
  const body = [
    "## Probe",
    "<script>alert(1)</script>",
    '<img src="x" onerror="alert(2)">',
    "[x](javascript:alert(3)) and <b>bold</b>",
    "<svg onload=alert(4)><circle r=1></circle></svg>",
    "| a <img src=y onerror=alert(5)> |\n|---|\n| <script>alert(6)</script> |",
  ].join("\n\n");
  render(<Document body={body} />);
  expect(document.querySelector("script")).toBeNull();
  expect(document.querySelectorAll(".prose img").length).toBe(2);
  for (const img of document.querySelectorAll(".prose img")) expect(img.hasAttribute("onerror")).toBe(false);
  expect(document.querySelector("[onload]")).toBeNull();
  expect(document.querySelector('a[href^="javascript"]')).toBeNull();
  expect(document.querySelector(".prose b")?.textContent).toBe("bold");
  expect(alert).not.toHaveBeenCalled();
  expect(clean('<img src="x" onerror="alert(2)">')).toBe('<img src="x">');
});

test("headings take the outline's anchors, a table is a table, and a check block reads under its name", () => {
  const body = [
    "Lead with a [[projects/bp/designs/system|link]].",
    "## Finish line",
    "| a | b |\n|---|---|\n| 1 | 2 |",
    "```check shell {name=api-routes expect=0 severity=warn}\ncurl x\n```",
    "### Deep `one`",
    "text",
  ].join("\n\n");
  const outline = [
    { depth: 2, heading: "Finish line", anchor: "finish-line" },
    { depth: 3, heading: "Deep `one`", anchor: "deep-one" },
  ];
  expect(outlineOf(body)).toEqual(outline);
  render(<Document body={body} outline={outline} />);
  expect(document.getElementById("finish-line")?.tagName).toBe("H2");
  expect(document.getElementById("deep-one")?.innerHTML).toBe("Deep <code>one</code>");
  expect(document.querySelectorAll(".prose table td")).toHaveLength(2);
  expect(document.querySelector("figure.check figcaption")?.textContent).toBe("api-routes · check shell · warn");
  expect(document.querySelector("figure.check code")?.textContent).toBe("curl x");
  expect(screen.getByRole("link", { name: "link" }).getAttribute("href")).toBe("/node/projects/bp/designs/system");
  const links = screen.getByRole("navigation", { name: "outline" }).querySelectorAll("a");
  expect(Array.from(links).map((a) => [a.getAttribute("href"), a.className])).toEqual([
    ["#finish-line", ""],
    ["#deep-one", "d3"],
  ]);
  fireEvent.click(links[1]);
  expect(links[1].getAttribute("aria-current")).toBe("location");
  expect(window.location.hash).toBe("#deep-one");
});

test("a folded body opens when the outline jumps into it", () => {
  render(<Document body={"## A\n\nx\n\n## B\n\ny"} fold="Body" />);
  const fold = document.querySelector("details.fold") as HTMLDetailsElement;
  expect(fold.open).toBe(false);
  fireEvent.click(screen.getByRole("link", { name: "B" }));
  expect(fold.open).toBe(true);
});

test("each diagram follows the block that cites it; one the body never cites follows the body", () => {
  const body = "## Context\n\n*Diagram [`ctx`](/projects/bp/designs/system/diagrams/ctx.html).*\n\nAfter.\n\n## Next\n\nText.";
  const plan = blocks(body, outlineOf(body), ["ctx", "other"]);
  const shape = plan.map((b) => (b.kind === "figure" ? `${b.name} in ${b.section ?? "no section"}` : b.kind));
  expect(shape).toEqual(["heading", "html", "ctx in context", "html", "heading", "html", "other in no section"]);
  expect(plan[1].kind === "html" && plan[1].html).toContain('href="#fig-ctx"');
});

// No `<style>` here, and the `<script>` comes last: happy-dom's parser drops what follows a raw-text element inside
// an `<svg>`. The design-page screenshot run proves the renderer's `<style>` survives in a browser.
const SVG =
  '<svg class="bp" viewBox="0 0 10 10"><metadata id="x-index">{"a":1}</metadata>' +
  '<g class="node" data-id="a" onclick="alert(2)"><rect class="box"></rect></g>' +
  '<g class="node" data-id="b"><rect></rect></g><g class="node focus" data-id="c"><rect></rect></g>' +
  '<g class="edge" data-id="a-b" data-from="a" data-to="b"></g><g class="label" data-id="l1" data-of="a-b"></g>' +
  '<g class="group" data-id="g"></g><script>alert(1)</script></svg>';

test("a diagram inlines its sanitised SVG; hover focuses and dims, click selects and marks the citing section", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(SVG, { status: 200 })));
  render(<Document body={"## Context\n\nSee diagrams/ctx for the parts.\n"} diagrams={[{ name: "ctx", title: "Context", src: "/api/x" }]} />);
  await waitFor(() => expect(document.querySelector("figure.fig svg")).toBeTruthy());
  const svg = document.querySelector("figure.fig svg")!;
  expect(svg.querySelector("script")).toBeNull();
  expect(svg.querySelector("[onclick]")).toBeNull();
  expect(svg.querySelector("metadata")?.textContent).toBe('{"a":1}');
  expect(svg.getAttribute("style")).toContain("color-scheme: dark");
  const part = (id: string) => svg.querySelector(`[data-id="${id}"]`)!;
  const dim = () => ["a", "b", "c", "a-b", "l1", "g"].filter((id) => part(id).classList.contains("dim"));
  fireEvent.mouseOver(part("a").querySelector("rect")!);
  expect(part("a").classList.contains("focus")).toBe(true);
  expect(dim()).toEqual(["c"]);
  fireEvent.mouseLeave(svg.parentElement!);
  expect([part("a").classList.contains("focus"), part("c").classList.contains("focus")]).toEqual([false, true]);
  expect(dim()).toEqual([]);
  fireEvent.click(part("l1"));
  expect(svg.querySelectorAll(".selected")).toHaveLength(1);
  expect(part("a-b").classList.contains("selected")).toBe(true);
  expect(screen.getByRole("link", { name: "Context" }).getAttribute("aria-current")).toBe("location");
});

test("the largest design body in the converted vault renders as a document", () => {
  const t0 = performance.now();
  render(<Document body={large} />);
  const ms = performance.now() - t0;
  console.info(`render research-stage.md (${large.length} bytes): ${ms.toFixed(0)} ms`);
  expect(document.querySelectorAll(".prose h2").length).toBe(outlineOf(large).filter((h) => h.depth === 2).length);
  expect(ms).toBeLessThan(3000);
});
