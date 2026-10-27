// The command chip (19, principle 4): the tool's own string, monospace, in the accent colour; a click copies it.
import type { ReactNode } from "react";
import { copy } from "./keys";

export function Command({ text }: { text: string }) {
  // A full id is longer than the column, so a line breaks at a space or after a `/` (a `<wbr>`), never inside a word or
  // an id's segment: each is one box, which breaks inside only when it alone overflows the line. The break is display
  // only: the copied text and `data-command` stay whole.
  const tokens = text.match(/[^/ ]*\/|[^/ ]+| +/g) ?? [];
  const parts = tokens.flatMap((t, i): ReactNode[] => {
    if (t.startsWith(" ")) return [t];
    const seg = <span key={i} className="seg">{t}</span>;
    return t.endsWith("/") && i < tokens.length - 1 ? [seg, <wbr key={`w${i}`} />] : [seg];
  });
  return (
    <button type="button" className="cmd" title="copy" onClick={(e) => (e.stopPropagation(), copy(text))}>
      {parts}
    </button>
  );
}
