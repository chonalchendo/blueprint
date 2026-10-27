// The path is the hierarchy (18, Interfaces 1; 32, design change 6): the URL names the page, the crumb trail is the
// path, and the browser's back button is the way up. Few routes, so a parser and the history API serve without a
// router library.
import { type MouseEvent, type ReactNode, useSyncExternalStore } from "react";

export type Route =
  | { page: "projects" }
  | { page: "home"; project: string }
  | { page: "moving"; project: string }
  | { page: "change"; project: string; id: string }
  | { page: "node"; id: string }
  | { page: "search"; q: string }
  | { page: "none"; path: string; project?: string };

/** The route of a location's path and query. */
export function route(path: string, search = ""): Route {
  let parts: string[];
  try {
    parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  } catch {
    // `decodeURIComponent` throws a `URIError` on a malformed escape (`%E0`): the path names no page.
    return { page: "none", path };
  }
  if (parts.length === 0) return { page: "projects" };
  // No id holds a `.` or `..` segment; the browser resolves one in the API path and fetches another route.
  const dots = (id: string) => id.split("/").some((s) => s === "." || s === "..");
  if (parts[0] === "node" && parts.length > 1) {
    const id = parts.slice(1).join("/");
    return dots(id) ? { page: "none", path } : { page: "node", id };
  }
  if (parts.length > 2 && parts[1] === "change") {
    const id = parts.slice(2).join("/");
    return dots(id) ? { page: "none", path, project: parts[0] } : { page: "change", project: parts[0], id };
  }
  if (parts[0] === "search" && parts.length === 1) return { page: "search", q: new URLSearchParams(search).get("q") ?? "" };
  if (parts.length === 1) return { page: "home", project: parts[0] };
  if (parts.length === 2 && parts[1] === "moving") return { page: "moving", project: parts[0] };
  return { page: "none", path, project: parts[0] };
}

/** The project an id carries: `projects/<project>/…`; a `shared/` id has none. */
export function projectOf(id: string): string | undefined {
  const parts = id.split("/");
  return parts[0] === "projects" ? parts[1] : undefined;
}

/** The Change page of an issue (18, Change): the page a Moving row of an issue opens. */
export function changePath(project: string, id: string): string {
  return `/${project}/change/${id}`;
}

/** The page path of a node; the full Design and Milestone pages take their own routes later (32, issue 5). */
export function nodePath(id: string): string {
  return `/node/${id}`;
}

const subscribers = new Set<() => void>();

function subscribe(fn: () => void): () => void {
  subscribers.add(fn);
  window.addEventListener("popstate", fn);
  return () => {
    subscribers.delete(fn);
    window.removeEventListener("popstate", fn);
  };
}

const location = () => window.location.pathname + window.location.search;

/** Moves to `to` as a link does, and keeps the step in history. */
export function navigate(to: string): void {
  if (to === location()) return;
  window.history.pushState(null, "", to);
  window.scrollTo?.(0, 0);
  for (const fn of subscribers) fn();
}

/** The current route; the component re-renders on each move. */
export function useRoute(): Route {
  const at = useSyncExternalStore(subscribe, location);
  const q = at.indexOf("?");
  return q < 0 ? route(at) : route(at.slice(0, q), at.slice(q));
}

/** An in-app link: a plain click moves without a reload; a modified click opens as the browser would. */
export function Link({ to, className, children }: { to: string; className?: string; children: ReactNode }) {
  const click = (e: MouseEvent<HTMLAnchorElement>) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    navigate(to);
  };
  return (
    <a href={to} className={className} onClick={click}>
      {children}
    </a>
  );
}
