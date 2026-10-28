// What the view adds to a diagram (28, design change 12; 32, design change 8): classes only, never a redraw. The SVG
// is the renderer's; its style draws `focus`, `dim` and `selected` with 450 ms transitions. Hover a node: `focus` on
// it, and `dim` on every part that is not it, its edges, their other ends or their labels. Click: `selected`. The
// view keeps its own toggles rather than attach `viewer.js`, whose pan, panel and hash a page has no use for; when
// the pointer leaves, the focus returns to the selection, as the viewer's does.

/** Each part of the drawing: a node, an edge, a label, a note or a group, by `data-id`. */
function parts(svg: Element): Element[] {
  return Array.from(svg.querySelectorAll("[data-id]"));
}

/** The ids that stay at full ink while `id` has focus: itself, for a node its edges and their ends, and for an edge
 * its ends. */
function keep(svg: Element, id: string): Set<string> {
  const out = new Set([id]);
  for (const e of svg.querySelectorAll("[data-from][data-to]")) {
    const [from, to] = [e.getAttribute("data-from"), e.getAttribute("data-to")];
    const eid = e.getAttribute("data-id");
    if (from === id || to === id || eid === id) [eid, from, to].forEach((x) => x && out.add(x));
  }
  return out;
}

/** The view's own focus mark, so a part the spec drew focal keeps its `focus` when the pointer leaves. */
const MINE = "data-view-focus";

/** Focuses the part `id`, or clears the focus with `null`. */
export function focus(svg: Element, id: string | null): void {
  const kept = id ? keep(svg, id) : null;
  for (const p of parts(svg)) {
    const of = p.getAttribute("data-of") ?? p.getAttribute("data-id")!;
    // A group frames its members; dimming it would grey the members' paper too.
    p.classList.toggle("dim", !!kept && !kept.has(of) && !p.classList.contains("group"));
    if (p.hasAttribute(MINE)) (p.classList.remove("focus"), p.removeAttribute(MINE));
  }
  const el = id ? parts(svg).find((p) => p.getAttribute("data-id") === id) : undefined;
  if (el && !el.classList.contains("focus")) (el.classList.add("focus"), el.setAttribute(MINE, ""));
}

/** Selects the part `id`: a label selects what it labels. The rest lose `selected`. */
export function select(svg: Element, id: string | null): void {
  const all = parts(svg);
  const hit = id ? all.find((p) => p.getAttribute("data-id") === id) : undefined;
  const target = hit?.getAttribute("data-of") ?? id;
  for (const p of all) p.classList.toggle("selected", p.getAttribute("data-id") === target);
}

/** The part under an event's target, if any. */
export function partOf(target: EventTarget | null): string | null {
  return target instanceof Element ? (target.closest("[data-id]")?.getAttribute("data-id") ?? null) : null;
}
