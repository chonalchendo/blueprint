// The command chip (19, principle 4): the tool's own string, monospace, in the accent colour; a click copies it.
import { copy } from "./keys";

export function Command({ text }: { text: string }) {
  return (
    <button type="button" className="cmd" title="copy" onClick={(e) => (e.stopPropagation(), copy(text))}>
      {text}
    </button>
  );
}
