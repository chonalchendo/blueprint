// Change (18, Change; 32, design change 5; mock `change-0411.html`): one issue, top to bottom the gate strip, the
// stack rail, what waits outside it, the deltas, the diff, then the one command or the refusal `land` would give.
// Every word comes from the server. The command slot reads `change.command` and nothing else (18, check 6): titles,
// notes, details and the diff are text React escapes, never markup and never a chip.
import { useState } from "react";
import { useApi } from "../api";
import { Command } from "../Command";
import { many, words } from "../intents";
import { Loaded } from "../Loaded";
import { Link, changePath, nodePath } from "../router";
import type { Change as ChangeObject } from "../types/Change";
import type { Diff as DiffObject } from "../types/Diff";
import { forgeWords } from "./Home";

/** The API path of an issue's Change page; each segment is encoded, as the node page encodes its id. */
const api = (project: string, id: string) => `${project}/change/${id.split("/").map(encodeURIComponent).join("/")}`;

const tail = (id: string) => id.split("/").slice(-1)[0];

export function Change({ project, id }: { project: string; id: string }) {
  const load = useApi<ChangeObject>(api(project, id));
  return <Loaded load={load}>{(c) => <ChangeView c={c} project={project} />}</Loaded>;
}

export function ChangeView({ c, project }: { c: ChangeObject; project: string }) {
  const pr = c.pull_request;
  return (
    <article className="change">
      <h1>
        <span className="id">{tail(c.id)}</span> {c.title ?? "untitled"}
      </h1>
      <p className="sub">
        <Link to={nodePath(c.id)} className="id">
          {c.id}
        </Link>{" "}
        · {c.intent ? words(c.intent) : "no intent"}
        {c.note ? `: ${c.note}` : ""} · {c.head} · {forgeWords(c)}
      </p>
      <h2 className="group">
        Gates<span className="n">{pr ? `${c.gates.filter((g) => g.state === "pass").length} of ${c.gates.length} pass` : "no pull request"}</span>
      </h2>
      {c.gates.length === 0 ? (
        <p className="empty">nothing to gate: {pr ? "no gate read" : "no pull request"}</p>
      ) : (
        <div className="gates">
          {c.gates.map((g) => (
            <div className="gate" key={g.name}>
              <span className="k">{g.name}</span>
              <span className={g.state === "pass" ? "ok" : g.state === "fail" ? "bad" : "na"}>{g.state}</span>
              <span className="detail">{g.detail}</span>
            </div>
          ))}
        </div>
      )}
      <h2 className="group">Stack</h2>
      {c.stack.length <= 1 ? (
        <p className="empty">not stacked</p>
      ) : (
        <ol className="stack">
          {c.stack.map((l) => (
            <li key={l.id} className={l.current ? "layer this" : "layer"}>
              <span className="n">{l.layer}</span>
              {l.current ? <span className="id">{tail(l.id)}</span> : <Link to={changePath(project, l.id)} className="id">{tail(l.id)}</Link>}
              <span className="sub">{l.current ? `${l.state} · this change` : l.state}</span>
            </li>
          ))}
        </ol>
      )}
      {c.waits_for.length > 0 && (
        <p className="sub waits">
          {many(c.waits_for.length, "issue")} wait{c.waits_for.length === 1 ? "s" : ""} for this to land:{" "}
          {c.waits_for.map((w, i) => (
            <span key={w}>
              {i > 0 ? ", " : ""}
              <Link to={nodePath(w)} className="id">
                {tail(w)}
              </Link>
            </span>
          ))}
        </p>
      )}
      <h2 className="group">
        Proposed deltas<span className="n">{c.deltas.length}</span>
      </h2>
      {c.deltas.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ul className="deltas">
          {c.deltas.map((d) => (
            <li key={d.id}>
              <Link to={nodePath(d.id)} className="id">
                {tail(d.id)}
              </Link>{" "}
              → <span className="id">{d.target}</span> <span className="sub">{d.state}</span>
            </li>
          ))}
        </ul>
      )}
      <h2 className="group">Diff</h2>
      <DiffView diff={c.diff} path={api(project, c.id)} pr={pr?.number} url={pr?.url} />
      <h2 className="group">Land</h2>
      <div className="land">
        {c.command ? (
          <Command text={c.command} />
        ) : c.refusal ? (
          <p className="refusal">
            refused <span className="id">{c.refusal.code}</span>: <span className="fix">{c.refusal.fix}</span>
          </p>
        ) : (
          <p className="wait">{c.wait}</p>
        )}
      </div>
    </article>
  );
}

/** The file stat, and the text behind one disclosure, read on its first opening. */
function DiffView({ diff, path, pr, url }: { diff: DiffObject; path: string; pr?: number; url?: string }) {
  const [open, setOpen] = useState(false);
  if (pr === undefined) return <p className="empty">no pull request</p>;
  const add = diff.files.reduce((n, f) => n + f.additions, 0);
  const del = diff.files.reduce((n, f) => n + f.deletions, 0);
  const forge = url ? <a href={url}>pull request {pr}</a> : `pull request ${pr}`;
  if (diff.files.length === 0) {
    return (
      <p className="empty">
        no file stat for {forge}: start bp-view with --clone at the project's clone
      </p>
    );
  }
  return (
    <div className="diff">
      <p>
        <span className="ok">+{add}</span> / <span className="bad">−{del}</span> across {many(diff.files.length, "file")} in {forge}
      </p>
      <table className="files">
        <tbody>
          {diff.files.map((f) => (
            <tr key={f.path}>
              <td className="id">{f.path}</td>
              <td className="ok">+{f.additions}</td>
              <td className="bad">−{f.deletions}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <details className="sub" onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
        <summary>diff text</summary>
        {open && <DiffText path={path} />}
      </details>
    </div>
  );
}

function DiffText({ path }: { path: string }) {
  const load = useApi<ChangeObject>(`${path}?diff=1`);
  return (
    <Loaded load={load}>
      {(c) => (
        <>
          <pre className="body">{c.diff.text ?? "no text"}</pre>
          {c.diff.truncated && <p className="sub">cut at 200 KB; git diff in the clone shows the rest</p>}
        </>
      )}
    </Loaded>
  );
}
