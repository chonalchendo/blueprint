// Keyboard first (19, principle 6). A list row is any element with `data-row`: `data-href` is where `enter` opens
// it, `data-command` what `c` copies. The focused row is the document's focus, so no state is kept here.

/** The rows a reader can see: not inside a closed disclosure. */
export function rows(root: ParentNode = document): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>("[data-row]")).filter(
    (r) => !r.parentElement?.closest("details:not([open])"),
  );
}

/** Copies `text` to the clipboard and tells the frame, which says so. */
export function copy(text: string): void {
  void navigator.clipboard?.writeText(text);
  window.dispatchEvent(new CustomEvent("bp-copied", { detail: text }));
}

type Actions = { open: (href: string) => void; palette: () => void };

/** The handler the frame installs on the window. */
export function onKey(e: KeyboardEvent, act: Actions): void {
  const t = e.target as HTMLElement | null;
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
  if ((e.key === "k" && (e.metaKey || e.ctrlKey)) || (e.key === "/" && !e.metaKey && !e.ctrlKey)) {
    e.preventDefault();
    act.palette();
    return;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const all = rows();
  const at = all.findIndex((r) => r === document.activeElement);
  if (e.key === "j" || e.key === "k") {
    if (all.length === 0) return;
    e.preventDefault();
    const next = at < 0 ? 0 : Math.min(Math.max(at + (e.key === "j" ? 1 : -1), 0), all.length - 1);
    all[next].focus();
    all[next].scrollIntoView?.({ block: "nearest" });
  } else if (e.key === "Enter" && at >= 0 && all[at].dataset.href) {
    e.preventDefault();
    act.open(all[at].dataset.href!);
  } else if (e.key === "c" && at >= 0 && all[at].dataset.command) {
    e.preventDefault();
    copy(all[at].dataset.command!);
  }
}
