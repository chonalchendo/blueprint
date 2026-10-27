// A body as a document (19, principle 9; 32, design change 8): prose at 72 ch, each diagram inline after the block
// that cites it, a check block under its name, and an outline fixed at the right from 1280 px. Every node page,
// the Milestone page and the Design page read their bodies through it. With `fold`, the body sits behind one
// disclosure (18, Disclosure rules) and the outline, a link or a figure opens it.
import { type MouseEvent, type ReactNode, createElement, useEffect, useMemo, useRef, useState } from "react";
import { navigate } from "../router";
import type { Heading } from "../types/Heading";
import { focus, partOf, select } from "./diagram";
import { type Block, blocks, cleanSvg, outlineOf } from "./markdown";

/** A diagram the body may cite: its name, its title and the URL of its SVG. */
export type Figure = { name: string; title: string; src: string };

type Props = { body: string; outline?: Heading[]; diagrams?: Figure[]; fold?: string; children?: ReactNode };

/** `children` sit above the body, inside the document, so their `#` links open the fold as the body's do. */
export function Document({ body, outline, diagrams = [], fold, children }: Props) {
  const anchors = useMemo(() => outline ?? outlineOf(body), [body, outline]);
  const names = useMemo(() => diagrams.map((d) => d.name), [diagrams]);
  const plan = useMemo(() => blocks(body, anchors, names), [body, anchors, names]);
  const [current, setCurrent] = useState<string>();
  const folded = useRef<HTMLDetailsElement>(null);
  const nav = useRef<HTMLElement>(null);

  /** Opens the fold, then scrolls to the element `id` and marks its section in the outline. */
  const go = (id: string) => {
    if (folded.current) folded.current.open = true;
    const el = document.getElementById(id);
    if (!el) return;
    el.scrollIntoView?.({ block: "start" });
    // A figure marks the section that cites it.
    const fig = plan.find((b) => b.kind === "figure" && `fig-${b.name}` === id);
    const section = fig?.kind === "figure" ? fig.section : id;
    if (anchors.some((a) => a.anchor === section)) setCurrent(section);
    window.history.replaceState(null, "", `#${id}`);
  };
  // A link to a section or a figure, as the URL holds it on load.
  useEffect(() => {
    const id = decodeURIComponent(window.location.hash.slice(1));
    if (id) go(id);
    // Once per body: a later hash change is the page's own.
  }, [body]);
  useEffect(() => nav.current?.querySelector("[aria-current]")?.scrollIntoView?.({ block: "nearest" }), [current]);

  const click = (e: MouseEvent) => {
    const a = (e.target as Element).closest("a");
    const href = a?.getAttribute("href") ?? "";
    if (href.startsWith("#")) (e.preventDefault(), go(decodeURIComponent(href.slice(1))));
    else if (href.startsWith("/") && !href.startsWith("//") && e.button === 0 && !(e.metaKey || e.ctrlKey || e.shiftKey)) {
      e.preventDefault();
      navigate(href);
    }
  };
  const prose = (
    <div className="prose">
      {plan.map((b, i) => (
        <Part key={i} b={b} diagrams={diagrams} pick={(s) => s && setCurrent(s)} />
      ))}
    </div>
  );
  return (
    <div className="doc" onClick={click}>
      {children}
      {fold ? (
        <details className="fold" ref={folded}>
          <summary>{fold}</summary>
          {prose}
        </details>
      ) : (
        prose
      )}
      {anchors.length > 0 && (
        <nav className="outline" aria-label="outline" ref={nav}>
          {anchors.map((h) => (
            <a
              key={h.anchor}
              href={`#${h.anchor}`}
              className={h.depth === 3 ? "d3" : undefined}
              aria-current={current === h.anchor ? "location" : undefined}
              onClick={(e) => (e.preventDefault(), go(h.anchor))}
            >
              {h.heading.replace(/`/g, "")}
            </a>
          ))}
        </nav>
      )}
    </div>
  );
}

function Part({ b, diagrams, pick }: { b: Block; diagrams: Figure[]; pick: (section?: string) => void }) {
  switch (b.kind) {
    case "html":
      return <div className="md" dangerouslySetInnerHTML={{ __html: b.html }} />;
    case "heading":
      return createElement(`h${b.depth}`, { id: b.anchor, dangerouslySetInnerHTML: { __html: b.html } });
    case "check":
      return (
        <figure className="check">
          <figcaption>
            <span className="id">{b.name ?? "unnamed"}</span> · check {b.runner} · {b.severity}
          </figcaption>
          <pre>
            <code>{b.text}</code>
          </pre>
        </figure>
      );
    case "figure": {
      const d = diagrams.find((x) => x.name === b.name);
      return d ? <Diagram d={d} pick={() => pick(b.section)} /> : null;
    }
  }
}

/** One diagram: the SVG the renderer wrote, sanitised and inlined, with the view's classes on hover and click. */
export function Diagram({ d, pick }: { d: Figure; pick: () => void }) {
  const [svg, setSvg] = useState<{ src: string; text?: string; why?: string }>({ src: d.src });
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    fetch(d.src)
      .then(async (r) => (r.ok ? { text: cleanSvg(await r.text()) } : { why: `the server answered ${r.status}` }))
      .catch((e: unknown) => ({ why: e instanceof Error ? e.message : String(e) }))
      .then((got) => live && setSvg({ src: d.src, ...got }));
    return () => {
      live = false;
    };
  }, [d.src]);
  const el = () => box.current?.querySelector("svg") ?? null;
  // The view is dark (19), so the drawing is too, whatever the system asks.
  useEffect(() => el()?.style.setProperty("color-scheme", "dark"), [svg]);
  const text = svg.src === d.src ? svg.text : undefined;
  const why = svg.src === d.src ? svg.why : undefined;
  return (
    <figure className="fig" id={`fig-${d.name}`}>
      <div
        ref={box}
        className="svg"
        onMouseOver={(e) => el() && focus(el()!, partOf(e.target))}
        onMouseLeave={() => el() && focus(el()!, null)}
        onClick={(e) => (el() && select(el()!, partOf(e.target)), pick())}
        dangerouslySetInnerHTML={{ __html: text ?? "" }}
      />
      {!text && <p className="sub">{why ? `diagram not read: ${why}` : "reading the diagram"}</p>}
      <figcaption>
        <span className="id">{d.name}</span> · {d.title}
      </figcaption>
    </figure>
  );
}
