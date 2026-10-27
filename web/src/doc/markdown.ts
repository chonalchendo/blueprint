// The vault's markdown as the page shows it (19, principle 9): headings as written, tables as tables, a fenced
// `check` block as code under its name, and each diagram after the block that cites it. A body is untrusted text, so
// `marked` renders it (CommonMark and GFM) and every HTML string it makes passes DOMPurify before the page holds it.
// Headings and check blocks are blocks of their own, so the page gives each heading its outline anchor and each
// check its label as React text.
import DOMPurify from "dompurify";
import { Marked, type Token, type TokenizerAndRendererExtension, type Tokens } from "marked";
import type { Heading } from "../types/Heading";

export type Block =
  | { kind: "html"; html: string }
  | { kind: "heading"; depth: number; html: string; anchor: string }
  | { kind: "check"; name?: string; runner: string; severity: string; text: string }
  | { kind: "figure"; name: string; section?: string };

/** `[[id]]` and `[[id|label]]`, the vault's links (12), as links to the node page. */
const wikilink: TokenizerAndRendererExtension = {
  name: "wikilink",
  level: "inline",
  start: (src) => src.indexOf("[["),
  tokenizer(src) {
    const m = /^\[\[([^\]|\n]+)(?:\|([^\]\n]+))?\]\]/.exec(src);
    if (m) return { type: "wikilink", raw: m[0], target: m[1].trim(), label: (m[2] ?? m[1]).trim() };
  },
  renderer: (t) => `<a class="wl" href="/node/${encodeURI(t.target)}">${escape(t.label)}</a>`,
};

/** A root-relative link into the vault opens the node page; one to a diagram's page jumps to its figure. */
function retarget(t: Token): void {
  if (t.type !== "link" && t.type !== "image") return;
  const m = /^\/((?:projects|shared)\/[^?#]*?)(\.md|\.html)?(?:[?#].*)?$/.exec(t.href);
  if (!m) return;
  const diagram = m[2] === ".html" ? /\/diagrams\/([^/]+)$/.exec(m[1]) : null;
  if (diagram) t.href = `#fig-${diagram[1]}`;
  else if (m[2] !== ".html") t.href = `/node/${m[1]}`;
}

const md = new Marked({ gfm: true, extensions: [wikilink], walkTokens: retarget });

function escape(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** HTML from a body, made safe: no script, no event handler, no `javascript:` URL. */
export function clean(html: string): string {
  return DOMPurify.sanitize(html);
}

/** A diagram's SVG, made safe with the SVG profile: its `<style>`, `<metadata>` and `data-*` attributes stay. */
export function cleanSvg(svg: string): string {
  return DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } });
}

/** Markdown with no block structure of the page's own: a TL;DR, a lead. */
export function html(text: string): string {
  return clean(md.parse(text, { async: false }));
}

/** The anchor rule the server's outline uses (`bp-read`, design.rs): lower case, each run of other characters one
 * `-`, then `-2`, `-3` on a repeat. A node page has no outline object, so it derives its own. */
export function outlineOf(body: string): Heading[] {
  const seen = new Map<string, number>();
  return md
    .lexer(body)
    .filter((t): t is Tokens.Heading => t.type === "heading" && (t.depth === 2 || t.depth === 3))
    .map((t) => {
      const slug = t.text.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-+|-+$/g, "") || "section";
      const n = (seen.get(slug) ?? 0) + 1;
      seen.set(slug, n);
      return { depth: t.depth, heading: t.text, anchor: n === 1 ? slug : `${slug}-${n}` };
    });
}

/** The body as blocks. A `## ` or `### ` heading takes the first unused outline entry of its depth and text. A
 * diagram follows the first block that names `diagrams/<name>` or `<name>.html`; one the body never cites follows
 * the body. */
export function blocks(body: string, outline: Heading[], diagrams: string[] = []): Block[] {
  const tokens = md.lexer(body);
  // `lexer` alone skips the `walkTokens` option, which `parse` runs.
  md.walkTokens(tokens, retarget);
  const used = new Set<Heading>();
  const placed = new Set<string>();
  const out: Block[] = [];
  let pending: Token[] = [];
  let section: string | undefined;
  const flush = () => {
    if (pending.length === 0) return;
    out.push({ kind: "html", html: clean(md.parser(Object.assign(pending, { links: tokens.links }))) });
    pending = [];
  };
  for (const t of tokens) {
    if (t.type === "heading" && (t.depth === 2 || t.depth === 3)) {
      const h = outline.find((o) => !used.has(o) && o.depth === t.depth && o.heading === t.text);
      if (h) {
        flush();
        used.add(h);
        section = h.anchor;
        out.push({ kind: "heading", depth: t.depth, html: clean(md.parseInline(t.text, { async: false })), anchor: h.anchor });
        continue;
      }
    }
    if (t.type === "code" && /^check(\s|$)/.test(t.lang ?? "")) {
      flush();
      out.push(check(t as Tokens.Code));
      continue;
    }
    pending.push(t);
    const cited = diagrams.filter((n) => !placed.has(n) && cites(t.raw, n));
    if (cited.length) {
      flush();
      for (const name of cited) (placed.add(name), out.push({ kind: "figure", name, section }));
    }
  }
  flush();
  for (const name of diagrams.filter((n) => !placed.has(n))) out.push({ kind: "figure", name });
  return out;
}

function cites(raw: string, name: string): boolean {
  const n = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`diagrams/${n}(?![\\w-])|(?<![\\w-])${n}\\.html`).test(raw);
}

/** A fenced check block (15): ```` ```check <runner> {name=<n> severity=<s> ...} ````. */
function check(t: Tokens.Code): Block {
  const info = t.lang ?? "";
  const attrs = Object.fromEntries(
    [...(/\{([^}]*)\}/.exec(info)?.[1] ?? "").matchAll(/([\w-]+)=(\S+)/g)].map((m) => [m[1], m[2]]),
  );
  const runner = info.split(/\s+/)[1]?.replace(/\{.*/, "") || "check";
  return { kind: "check", name: attrs.name, runner, severity: attrs.severity ?? "error", text: t.text };
}
