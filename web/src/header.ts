// `Show`'s header is an open map in its exported type (`web/src/types/Show.ts`), so the fields every page reads are
// narrowed here, once, with a runtime check.
import type { Show } from "./types/Show";

export type Header = { id: string; kind: string; title?: string; status?: string };

/** The id, kind, title and status of a `show` object. A missing or non-text `kind` throws: the object is not one. */
export function header(show: Show): Header {
  const { id, kind, title, status } = show as Record<string, unknown>;
  if (typeof id !== "string" || typeof kind !== "string") {
    throw new Error(`not a show object: id ${typeof id}, kind ${typeof kind}`);
  }
  return { id, kind, title: text(title), status: text(status) };
}

function text(v: unknown): string | undefined {
  return typeof v === "string" ? v : undefined;
}

/** The header fields a page prints beside the narrowed four: scalars and lists of scalars, by key. */
export function facts(show: Show): [string, string][] {
  const skip = new Set(["id", "kind", "title", "status", "body", "lead", "hash", "record", "sections"]);
  const out: [string, string][] = [];
  for (const [k, v] of Object.entries(show as Record<string, unknown>)) {
    if (skip.has(k)) continue;
    if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") out.push([k, String(v)]);
    else if (Array.isArray(v)) out.push([k, v.map((x) => (typeof x === "object" ? JSON.stringify(x) : String(x))).join(", ")]);
    else if (v && typeof v === "object") out.push([k, JSON.stringify(v)]);
  }
  return out;
}
