// Search and jump (19, principle 6): `/` or ⌘K opens it over `/api/search`; `enter` opens the chosen row's node.
import { type KeyboardEvent, useEffect, useState } from "react";
import { read } from "./api";
import { nodePath } from "./router";
import type { Hit } from "./types/Hit";
import type { Search } from "./types/Search";

type Row = { id: string; kind: string; title: string; snippet?: string };

/** An id typed in full jumps without a search. */
const isId = (q: string) => /^(projects|shared)\/\S+$/.test(q.trim());

export function Palette({ close, open }: { close: () => void; open: (href: string) => void }) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [at, setAt] = useState(0);
  useEffect(() => {
    const terms = q.trim();
    if (!terms) return setHits([]);
    let live = true;
    const t = setTimeout(async () => {
      const l = await read<Search>(`search?q=${encodeURIComponent(terms)}`);
      if (live) setHits(l.state === "ok" ? l.data.rows : []);
    }, 120);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [q]);
  const rows: Row[] = [
    ...(isId(q) ? [{ id: q.trim(), kind: "id", title: "open by id" }] : []),
    ...hits.map((h) => ({ id: h.id, kind: h.kind, title: h.title ?? h.id, snippet: h.snippet })),
  ];
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") close();
    else if (e.key === "ArrowDown") (e.preventDefault(), setAt((a) => Math.min(a + 1, rows.length - 1)));
    else if (e.key === "ArrowUp") (e.preventDefault(), setAt((a) => Math.max(a - 1, 0)));
    else if (e.key === "Enter" && rows[at]) (e.preventDefault(), open(nodePath(rows[at].id)));
  };
  return (
    <div className="palette-back" onClick={close}>
      <div className="palette" role="dialog" aria-label="search and jump" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          aria-label="search or id"
          placeholder="search, or an id such as projects/bp/designs/system"
          value={q}
          onChange={(e) => (setQ(e.target.value), setAt(0))}
          onKeyDown={key}
        />
        <ul role="listbox">
          {q.trim() && rows.length === 0 && <li className="sub">nothing</li>}
          {rows.map((r, i) => (
            <li key={r.id} role="option" aria-selected={i === at} onMouseEnter={() => setAt(i)} onClick={() => open(nodePath(r.id))}>
              <span>
                <span className="id">{r.id}</span> <span className="sub">{r.kind}</span>
              </span>
              <span className="sub">{r.snippet ? <Snippet text={r.snippet} /> : r.title}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** A search snippet, its terms marked `«` and `»`, as text: a body never reaches the page as markup. */
export function Snippet({ text }: { text: string }) {
  return (
    <>
      {text.split(/(«[^»]*»)/).map((part, i) =>
        part.startsWith("«") ? <mark key={i}>{part.slice(1, -1)}</mark> : <span key={i}>{part}</span>,
      )}
    </>
  );
}
