// The frame (18, Interfaces 1; 19): the crumb trail is the route, the palette opens from anywhere, and the keys of
// 19, principle 6 work on every list.
import { Component, Fragment, type ReactNode, useEffect, useState } from "react";
import { onKey } from "./keys";
import { Palette } from "./Palette";
import { Link, type Route, navigate, projectOf, useRoute } from "./router";
import { Home } from "./pages/Home";
import { Moving } from "./pages/Moving";
import { Node } from "./pages/Node";
import { Projects } from "./pages/Projects";
import { SearchPage } from "./pages/Search";

/** The crumbs of a route: each a label and the path it opens. */
export function crumbs(r: Route): [string, string][] {
  const top: [string, string] = ["projects", "/"];
  switch (r.page) {
    case "projects":
      return [top];
    case "home":
      return [top, [r.project, `/${r.project}`]];
    case "moving":
      return [top, [r.project, `/${r.project}`], ["moving", `/${r.project}/moving`]];
    case "node": {
      const p = projectOf(r.id);
      return [top, ...(p ? [[p, `/${p}`] as [string, string]] : []), [r.id.split("/").slice(-1)[0], `/node/${r.id}`]];
    }
    case "search":
      return [top, [`search ${r.q}`, `/search?q=${encodeURIComponent(r.q)}`]];
    case "none":
      return [top, ...(r.project ? [[r.project, `/${r.project}`] as [string, string]] : []), [r.path, r.path]];
  }
}

export function App() {
  const r = useRoute();
  const [palette, setPalette] = useState(false);
  const [copied, setCopied] = useState("");
  useEffect(() => {
    const key = (e: KeyboardEvent) => onKey(e, { open: navigate, palette: () => setPalette(true) });
    let t: ReturnType<typeof setTimeout> | undefined;
    const said = (e: Event) => {
      setCopied((e as CustomEvent<string>).detail);
      clearTimeout(t);
      t = setTimeout(() => setCopied(""), 2000);
    };
    window.addEventListener("keydown", key);
    window.addEventListener("bp-copied", said);
    return () => (window.removeEventListener("keydown", key), window.removeEventListener("bp-copied", said));
  }, []);
  const trail = crumbs(r);
  return (
    <>
      <nav className="top" aria-label="crumbs">
        <Link to="/" className="brand">
          bp
        </Link>
        {trail.map(([label, to], i) => (
          <Fragment key={to}>
            <span className="sep">›</span>
            <Link to={to} className={i === trail.length - 1 ? "item on" : "item"}>
              {label}
            </Link>
          </Fragment>
        ))}
        <span className="right" role="status">
          {copied ? (
            <>
              copied <span className="id">{copied}</span>
            </>
          ) : (
            <>
              <span className="kbd">/</span> <span className="kbd">⌘K</span> search · jump by id ·{" "}
              <span className="kbd">j</span> <span className="kbd">k</span> rows · <span className="kbd">c</span> copy
            </>
          )}
        </span>
      </nav>
      <main>
        <Failed key={trail[trail.length - 1][1]}>
          <Page r={r} />
        </Failed>
      </main>
      {palette && (
        <Palette
          close={() => setPalette(false)}
          open={(href) => (setPalette(false), navigate(href))}
        />
      )}
    </>
  );
}

/** A page that throws shows the error in words, as `Loaded` shows a refusal, in place of a blank screen. The key
 * resets it on each move. */
class Failed extends Component<{ children: ReactNode }, { error?: Error }> {
  state: { error?: Error } = {};
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    const e = this.state.error;
    if (!e) return this.props.children;
    return (
      <div className="banner" role="alert">
        failed <span className="id">{e.name}</span>: {e.message}. Fix: go back, or open <Link to="/">projects</Link>
      </div>
    );
  }
}

function Page({ r }: { r: Route }) {
  switch (r.page) {
    case "projects":
      return <Projects />;
    case "home":
      return <Home project={r.project} />;
    case "moving":
      return <Moving project={r.project} />;
    case "node":
      return <Node id={r.id} />;
    case "search":
      return <SearchPage q={r.q} />;
    case "none":
      return (
        <>
          <h1>No page here yet</h1>
          <p className="sub">
            <span className="id">{r.path}</span> is a route a later build serves.
          </p>
        </>
      );
  }
}
